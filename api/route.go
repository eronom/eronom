package api

import (
	"eronom/route"
	"time"
)

var (
	startTime = time.Now()
)

// Routes registers all API routes onto the provided app instance.
func Routes(app *route.App) {
	// ── GET /api/hello ───────────────────────────────────────────────────
	app.GET("/hello", func(c *route.Ctx) error {
		return c.Status(200).JSON(route.H{
			"message":   "Hello from Eronom API (Fiber style)!",
			"timestamp": time.Now().Unix(),
			"server":    "Go",
		})
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
