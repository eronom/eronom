package api

import (
	"eronom/route"
	"sync"
	"time"
)

// Todo state for the API
type Todo struct {
	ID        int       `json:"id"`
	Text      string    `json:"text"`
	Done      bool      `json:"done"`
	CreatedAt time.Time `json:"createdAt"`
}

var (
	todos     = []Todo{}
	todoID    = 0
	todoMu    sync.Mutex
	startTime = time.Now()
)

// Setup registers all API routes onto the provided app instance.
// This allows main.go to keep its local "app" variable while splitting
// the route logic into this separate package.
func Routes(app *route.App) {
	// ── GET /api/hello ───────────────────────────────────────────────────
	app.GET("/hello", func(c *route.Ctx) error {
		return c.Status(200).JSON(route.H{
			"message":   "Hello from Eronom API (Fiber style)!",
			"timestamp": time.Now().Unix(),
			"server":    "Go",
		})
	})

	// ── GET /api/todos ───────────────────────────────────────────────────
	app.GET("/todos", func(c *route.Ctx) error {
		todoMu.Lock()
		defer todoMu.Unlock()
		return c.JSON(todos)
	})

	// ── POST /api/todos ──────────────────────────────────────────────────
	app.POST("/todos", func(c *route.Ctx) error {
		var body struct {
			Text string `json:"text"`
		}
		if err := c.BindJSON(&body); err != nil || body.Text == "" {
			return c.Status(400).JSON(route.H{"error": "text is required"})
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

		return c.Status(201).JSON(todo)
	})

	// ── DELETE /api/todos ────────────────────────────────────────────────
	app.DELETE("/todos", func(c *route.Ctx) error {
		var body struct {
			ID int `json:"id"`
		}
		if err := c.BindJSON(&body); err != nil {
			return c.Status(400).JSON(route.H{"error": "id is required"})
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
			return c.Status(404).JSON(route.H{"error": "todo not found"})
		}
		return c.JSON(route.H{"status": "deleted"})
	})

	// ── PATCH /api/todos ─────────────────────────────────────────────────
	app.PATCH("/todos", func(c *route.Ctx) error {
		var body struct {
			ID   int  `json:"id"`
			Done bool `json:"done"`
		}
		if err := c.BindJSON(&body); err != nil {
			return c.Status(400).JSON(route.H{"error": "id is required"})
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
			return c.Status(404).JSON(route.H{"error": "todo not found"})
		}
		return c.JSON(updated)
	})

	// ── GET /api/server-info ─────────────────────────────────────────────
	app.GET("/server-info", func(c *route.Ctx) error {
		return c.JSON(route.H{
			"goVersion":  "1.25",
			"framework":  "eronom",
			"version":    "0.1.0",
			"uptime":     time.Since(startTime).String(),
			"serverTime": time.Now().Format(time.RFC3339),
		})
	})
}
