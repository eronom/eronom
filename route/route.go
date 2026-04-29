package route

import (
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"sync"
)

// H is a shortcut for map[string]interface{}
type H map[string]interface{}

// Ctx represents the context of the current HTTP request, similar to Fiber's Ctx.
type Ctx struct {
	Writer  http.ResponseWriter
	Request *http.Request
	status  int
}

// Status sets the HTTP status code and returns the context for chaining.
func (c *Ctx) Status(status int) *Ctx {
	c.status = status
	return c
}

// JSON sends a JSON response.
func (c *Ctx) JSON(data interface{}) error {
	status := c.status
	if status == 0 {
		status = http.StatusOK
	}
	c.Writer.Header().Set("Content-Type", "application/json")
	c.Writer.Header().Set("Access-Control-Allow-Origin", "*")
	c.Writer.WriteHeader(status)
	return json.NewEncoder(c.Writer).Encode(data)
}

// SendString sends a plain text response.
func (c *Ctx) SendString(text string) error {
	status := c.status
	if status == 0 {
		status = http.StatusOK
	}
	c.Writer.Header().Set("Content-Type", "text/plain; charset=utf-8")
	c.Writer.WriteHeader(status)
	_, err := c.Writer.Write([]byte(text))
	return err
}

// BindJSON parses the request body as JSON.
func (c *Ctx) BindJSON(target interface{}) error {
	defer c.Request.Body.Close()
	return json.NewDecoder(c.Request.Body).Decode(target)
}

// HandlerFunc defines the signature for route handlers.
type HandlerFunc func(c *Ctx) error

// App is the main router that supports a Fiber/Hono-like API with chaining.
type App struct {
	mu     sync.RWMutex
	routes map[string]HandlerFunc
}

// NewApp creates a new App instance.
func NewApp() *App {
	return &App{
		routes: make(map[string]HandlerFunc),
	}
}

// Handle registers a handler and returns the App for chaining.
func (a *App) Handle(method, path string, h HandlerFunc) *App {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.routes[method+" "+path] = h
	return a
}

func (a *App) GET(path string, h HandlerFunc) *App    { return a.Handle("GET", path, h) }
func (a *App) POST(path string, h HandlerFunc) *App   { return a.Handle("POST", path, h) }
func (a *App) PUT(path string, h HandlerFunc) *App    { return a.Handle("PUT", path, h) }
func (a *App) DELETE(path string, h HandlerFunc) *App { return a.Handle("DELETE", path, h) }
func (a *App) PATCH(path string, h HandlerFunc) *App  { return a.Handle("PATCH", path, h) }

// ServeHTTP implements the http.Handler interface.
func (a *App) ServeHTTP(w http.ResponseWriter, req *http.Request) {
	path := strings.TrimPrefix(req.URL.Path, "/api")
	if path == "" {
		path = "/"
	}

	method := req.Method

	a.mu.RLock()
	handler, ok := a.routes[method+" "+path]
	a.mu.RUnlock()

	if ok {
		c := &Ctx{Writer: w, Request: req}
		if err := handler(c); err != nil {
			http.Error(w, err.Error(), http.StatusInternalServerError)
		}
		return
	}

	// 404
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusNotFound)
	json.NewEncoder(w).Encode(map[string]string{
		"error": fmt.Sprintf("API route not found: %s %s", method, path),
	})
}
