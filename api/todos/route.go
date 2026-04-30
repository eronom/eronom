package todos

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
	todos  = []Todo{}
	todoID = 0
	todoMu sync.Mutex
)

// Routes registers todos logic
func Routes(app *route.App) {
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
}
