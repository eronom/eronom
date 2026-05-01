package todos

import (
	"eronom/api/todos/store"
	"eronom/route"
)

// Routes registers all todo-related routes.
func Routes(app *route.App) {
	// ── GET /api/todos ───────────────────────────────────────────────────
	app.GET("/", func(c *route.Ctx) error {
		return c.JSON(store.GetAll())
	})

	// ── POST /api/todos ──────────────────────────────────────────────────
	app.POST("/", func(c *route.Ctx) error {
		var body struct {
			Text string `json:"text"`
		}
		if err := c.BindJSON(&body); err != nil || body.Text == "" {
			return c.Status(400).JSON(route.H{"error": "text is required"})
		}

		todo := store.Add(body.Text)
		return c.Status(201).JSON(todo)
	})

	// ── GET /api/todos/:id ───────────────────────────────────────────────
	app.GET("/:id", func(c *route.Ctx) error {
		id := c.Param("id")
		todo, found := store.GetByID(id)
		if !found {
			return c.Status(404).JSON(route.H{"error": "todo not found"})
		}
		return c.JSON(todo)
	})

	// ── DELETE /api/todos ────────────────────────────────────────────────
	app.DELETE("/", func(c *route.Ctx) error {
		var body struct {
			ID int `json:"id"`
		}
		if err := c.BindJSON(&body); err != nil {
			return c.Status(400).JSON(route.H{"error": "id is required"})
		}

		if !store.Delete(body.ID) {
			return c.Status(404).JSON(route.H{"error": "todo not found"})
		}
		return c.JSON(route.H{"status": "deleted"})
	})

	// ── PATCH /api/todos ─────────────────────────────────────────────────
	app.PATCH("/", func(c *route.Ctx) error {
		var body struct {
			ID   int  `json:"id"`
			Done bool `json:"done"`
		}
		if err := c.BindJSON(&body); err != nil {
			return c.Status(400).JSON(route.H{"error": "id is required"})
		}

		updated, found := store.Update(body.ID, body.Done)
		if !found {
			return c.Status(404).JSON(route.H{"error": "todo not found"})
		}
		return c.JSON(updated)
	})
}
