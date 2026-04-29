package api

import (
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"sync"
	"time"
)

// =============================================================================
// Eronom API Routes — Next.js-style API endpoints (/api/...)
// =============================================================================

// Router manages all registered API route handlers.
type Router struct {
	mu     sync.RWMutex
	routes map[string]http.HandlerFunc
}

// NewRouter creates a new API router.
func NewRouter() *Router {
	r := &Router{
		routes: make(map[string]http.HandlerFunc),
	}
	r.registerDefaults()
	return r
}

// Handle registers an API route handler. The path should NOT include "/api/" prefix.
// Example: router.Handle("GET /users", handler) → serves GET /api/users
func (r *Router) Handle(methodAndPath string, handler http.HandlerFunc) {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.routes[methodAndPath] = handler
}

// ServeHTTP dispatches incoming /api/* requests to the matching handler.
func (r *Router) ServeHTTP(w http.ResponseWriter, req *http.Request) {
	// Strip /api prefix
	path := strings.TrimPrefix(req.URL.Path, "/api")
	if path == "" {
		path = "/"
	}

	method := req.Method

	r.mu.RLock()
	defer r.mu.RUnlock()

	// Try exact method+path match first: "GET /users"
	key := method + " " + path
	if handler, ok := r.routes[key]; ok {
		handler(w, req)
		return
	}

	// Try any-method match: "/users"
	if handler, ok := r.routes[path]; ok {
		handler(w, req)
		return
	}

	// 404
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusNotFound)
	json.NewEncoder(w).Encode(map[string]string{
		"error": fmt.Sprintf("API route not found: %s %s", method, path),
	})
}

// --- JSON helpers for route handlers ---

// JSON sends a JSON response with the given status code.
func JSON(w http.ResponseWriter, status int, data interface{}) {
	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("Access-Control-Allow-Origin", "*")
	w.WriteHeader(status)
	json.NewEncoder(w).Encode(data)
}

// ReadJSON parses the request body as JSON into the target.
func ReadJSON(r *http.Request, target interface{}) error {
	defer r.Body.Close()
	return json.NewDecoder(r.Body).Decode(target)
}

// =============================================================================
// Default example routes — real use cases that JS can't do
// =============================================================================

// Todo represents a todo item (server-side state)
type Todo struct {
	ID        int       `json:"id"`
	Text      string    `json:"text"`
	Done      bool      `json:"done"`
	CreatedAt time.Time `json:"createdAt"`
}

var (
	todos  = []Todo{}
	todoID = 0
	todoMu sync.Mutex
)

func (r *Router) registerDefaults() {
	// ── GET /api/hello ───────────────────────────────────────────────────
	// Simple JSON response — like Next.js pages/api/hello.js
	r.Handle("GET /hello", func(w http.ResponseWriter, req *http.Request) {
		JSON(w, 200, map[string]interface{}{
			"message":   "Hello from Eronom APIsss!",
			"timestamp": time.Now().Unix(),
			"server":    "Go",
		})
	})

	// ── GET /api/todos ───────────────────────────────────────────────────
	// Returns all todos — server-side state that JS alone can't persist
	r.Handle("GET /todos", func(w http.ResponseWriter, req *http.Request) {
		todoMu.Lock()
		defer todoMu.Unlock()
		JSON(w, 200, todos)
	})

	// ── POST /api/todos ──────────────────────────────────────────────────
	// Creates a new todo — modifies server state
	r.Handle("POST /todos", func(w http.ResponseWriter, req *http.Request) {
		var body struct {
			Text string `json:"text"`
		}
		if err := ReadJSON(req, &body); err != nil || body.Text == "" {
			JSON(w, 400, map[string]string{"error": "text is required"})
			return
		}

		todoMu.Lock()
		todoID++
		todo := Todo{
			ID:        todoID,
			Text:      body.Text,
			Done:      false,
			CreatedAt: time.Now(),
		}
		todos = append(todos, todo)
		todoMu.Unlock()

		JSON(w, 201, todo)
	})

	// ── DELETE /api/todos ────────────────────────────────────────────────
	// Deletes a todo by ID — server-side mutation
	r.Handle("DELETE /todos", func(w http.ResponseWriter, req *http.Request) {
		var body struct {
			ID int `json:"id"`
		}
		if err := ReadJSON(req, &body); err != nil {
			JSON(w, 400, map[string]string{"error": "id is required"})
			return
		}

		todoMu.Lock()
		found := false
		for i, t := range todos {
			if t.ID == body.ID {
				todos = append(todos[:i], todos[i+1:]...)
				found = true
				break
			}
		}
		todoMu.Unlock()

		if !found {
			JSON(w, 404, map[string]string{"error": "todo not found"})
			return
		}
		JSON(w, 200, map[string]string{"status": "deleted"})
	})

	// ── PATCH /api/todos ─────────────────────────────────────────────────
	// Toggle done status — server-side state update
	r.Handle("PATCH /todos", func(w http.ResponseWriter, req *http.Request) {
		var body struct {
			ID   int  `json:"id"`
			Done bool `json:"done"`
		}
		if err := ReadJSON(req, &body); err != nil {
			JSON(w, 400, map[string]string{"error": "id is required"})
			return
		}

		todoMu.Lock()
		var updated *Todo
		for i := range todos {
			if todos[i].ID == body.ID {
				todos[i].Done = body.Done
				updated = &todos[i]
				break
			}
		}
		todoMu.Unlock()

		if updated == nil {
			JSON(w, 404, map[string]string{"error": "todo not found"})
			return
		}
		JSON(w, 200, updated)
	})

	// ── GET /api/server-info ─────────────────────────────────────────────
	// Server-only info that the browser can't know
	r.Handle("GET /server-info", func(w http.ResponseWriter, req *http.Request) {
		JSON(w, 200, map[string]interface{}{
			"goVersion":  "1.25",
			"framework":  "eronom",
			"version":    "0.1.0",
			"uptime":     time.Since(startTime).String(),
			"serverTime": time.Now().Format(time.RFC3339),
		})
	})
}

var startTime = time.Now()
