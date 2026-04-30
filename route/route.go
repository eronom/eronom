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
	params  map[string]string
	status  int
}

// Param returns the value of a path parameter.
func (c *Ctx) Param(name string) string {
	if c.params == nil {
		return ""
	}
	return c.params[name]
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

type routeEntry struct {
	method  string
	path    string
	handler HandlerFunc
}

// App is the main router that supports a Fiber/Hono-like API with chaining.
type App struct {
	mu     *sync.RWMutex
	routes *[]routeEntry
	prefix string
}

// NewApp creates a new App instance.
func NewApp() *App {
	return &App{
		mu:     &sync.RWMutex{},
		routes: &[]routeEntry{},
		prefix: "",
	}
}

// Handle registers a handler and returns the App for chaining.
func (a *App) Handle(method, path string, h HandlerFunc) *App {
	a.mu.Lock()
	defer a.mu.Unlock()

	// Clean up path and apply prefix
	fullPath := a.prefix + "/" + strings.Trim(path, "/")
	if fullPath == "//" {
		fullPath = "/"
	} else if fullPath != "/" {
		fullPath = strings.TrimSuffix(fullPath, "/")
	}

	*a.routes = append(*a.routes, routeEntry{method: method, path: fullPath, handler: h})
	return a
}

func (a *App) GET(path string, h HandlerFunc) *App    { return a.Handle("GET", path, h) }
func (a *App) POST(path string, h HandlerFunc) *App   { return a.Handle("POST", path, h) }
func (a *App) PUT(path string, h HandlerFunc) *App    { return a.Handle("PUT", path, h) }
func (a *App) DELETE(path string, h HandlerFunc) *App { return a.Handle("DELETE", path, h) }
func (a *App) PATCH(path string, h HandlerFunc) *App  { return a.Handle("PATCH", path, h) }

// Route mounts an existing sub-router (App) under a prefix.
func (a *App) Route(prefix string, subApp *App) *App {
	a.mu.Lock()
	defer a.mu.Unlock()
	for _, r := range *subApp.routes {
		fullPath := a.prefix + prefix + r.path
		if r.path == "/" {
			fullPath = a.prefix + prefix
		}
		*a.routes = append(*a.routes, routeEntry{
			method:  r.method,
			path:    fullPath,
			handler: r.handler,
		})
	}
	return a
}

// Group creates a sub-router that shares the same route list but with a prefix.
func (a *App) Group(prefix string, fns ...func(sub *App)) *App {
	group := &App{
		mu:     a.mu,
		routes: a.routes,
		prefix: a.prefix + prefix,
	}
	for _, fn := range fns {
		fn(group)
	}
	return group
}

// ServeHTTP implements the http.Handler interface.
func (a *App) ServeHTTP(w http.ResponseWriter, req *http.Request) {
	path := strings.TrimPrefix(req.URL.Path, "/api")
	if path == "" {
		path = "/"
	}

	method := req.Method

	a.mu.RLock()
	defer a.mu.RUnlock()

	for _, r := range *a.routes {
		if r.method == method {
			params, ok := matchPath(r.path, path)
			if ok {
				c := &Ctx{Writer: w, Request: req, params: params}
				if err := r.handler(c); err != nil {
					http.Error(w, err.Error(), http.StatusInternalServerError)
				}
				return
			}
		}
	}

	// 404
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusNotFound)
	json.NewEncoder(w).Encode(map[string]string{
		"error": fmt.Sprintf("API route not found: %s %s", method, path),
	})
}

func matchPath(pattern, path string) (map[string]string, bool) {
	patternParts := strings.Split(strings.Trim(pattern, "/"), "/")
	pathParts := strings.Split(strings.Trim(path, "/"), "/")

	if len(patternParts) != len(pathParts) {
		// Special case for root
		if pattern == "/" && path == "/" {
			return make(map[string]string), true
		}
		return nil, false
	}

	params := make(map[string]string)
	for i := 0; i < len(patternParts); i++ {
		if strings.HasPrefix(patternParts[i], ":") {
			params[patternParts[i][1:]] = pathParts[i]
		} else if patternParts[i] != pathParts[i] {
			return nil, false
		}
	}
	return params, true
}
