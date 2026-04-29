package api

import (
	"encoding/json"
	"fmt"
	"math"
	"math/rand"
	"os"
	"strings"
	"time"
)

// =============================================================================
// Erm Server-Side API — provides data loaders and helpers for .erm templates
// =============================================================================

// LoaderFunc is a function that returns data for a template.
// The returned map's keys become available as variables in the template's script context.
type LoaderFunc func(params map[string]string) (map[string]interface{}, error)

// Registry holds all registered API loaders and built-in helpers.
type Registry struct {
	loaders  map[string]LoaderFunc
	builtins map[string]interface{}
}

// NewRegistry creates a new API registry with built-in helpers pre-loaded.
func NewRegistry() *Registry {
	r := &Registry{
		loaders:  make(map[string]LoaderFunc),
		builtins: make(map[string]interface{}),
	}
	r.registerBuiltins()
	return r
}

// Register adds a named loader function to the registry.
// Templates can call this loader via the {#load "name"} directive.
func (r *Registry) Register(name string, fn LoaderFunc) {
	r.loaders[name] = fn
}

// GetLoader retrieves a registered loader by name.
func (r *Registry) GetLoader(name string) (LoaderFunc, bool) {
	fn, ok := r.loaders[name]
	return fn, ok
}

// GetBuiltins returns all built-in helper values/functions.
func (r *Registry) GetBuiltins() map[string]interface{} {
	return r.builtins
}

// Load executes a named loader and returns the resulting data map.
func (r *Registry) Load(name string, params map[string]string) (map[string]interface{}, error) {
	fn, ok := r.loaders[name]
	if !ok {
		return nil, fmt.Errorf("api: loader %q not found", name)
	}
	return fn(params)
}

// LoadAll runs all registered loaders (with nil params) and merges results.
func (r *Registry) LoadAll() map[string]interface{} {
	merged := make(map[string]interface{})
	// Copy builtins first
	for k, v := range r.builtins {
		merged[k] = v
	}
	return merged
}

// =============================================================================
// Built-in helpers available to every template
// =============================================================================

func (r *Registry) registerBuiltins() {
	now := time.Now()

	// ── Date & Time ──────────────────────────────────────────────────────
	r.builtins["__date"] = map[string]interface{}{
		"now":       now.Format(time.RFC3339),
		"year":      float64(now.Year()),
		"month":     float64(now.Month()),
		"day":       float64(now.Day()),
		"hour":      float64(now.Hour()),
		"minute":    float64(now.Minute()),
		"second":    float64(now.Second()),
		"timestamp": float64(now.Unix()),
		"iso":       now.Format("2006-01-02"),
		"time":      now.Format("15:04:05"),
		"weekday":   now.Weekday().String(),
	}

	// ── Math utilities ───────────────────────────────────────────────────
	r.builtins["__math"] = map[string]interface{}{
		"pi":      math.Pi,
		"e":       math.E,
		"sqrt2":   math.Sqrt2,
		"ln2":     math.Ln2,
		"ln10":    math.Ln10,
		"maxSafe": float64(9007199254740991), // Number.MAX_SAFE_INTEGER
	}

	// ── Environment helpers (safe subset) ────────────────────────────────
	envVars := map[string]interface{}{}
	// Only expose ERM_ prefixed env vars for safety
	for _, e := range os.Environ() {
		parts := strings.SplitN(e, "=", 2)
		if len(parts) == 2 && strings.HasPrefix(parts[0], "ERM_") {
			envVars[parts[0]] = parts[1]
		}
	}
	r.builtins["__env"] = envVars

	// ── App metadata ─────────────────────────────────────────────────────
	r.builtins["__app"] = map[string]interface{}{
		"framework": "eronom",
		"version":   "0.1.0",
		"mode":      "development",
	}

	// Register built-in loaders
	r.registerDefaultLoaders()
}

func (r *Registry) registerDefaultLoaders() {
	// "meta" loader — returns page metadata that templates can use for <title>, etc.
	r.Register("meta", func(params map[string]string) (map[string]interface{}, error) {
		title := params["title"]
		if title == "" {
			title = "Eronom App"
		}
		description := params["description"]
		if description == "" {
			description = "Built with Eronom"
		}
		return map[string]interface{}{
			"title":       title,
			"description": description,
			"generator":   "eronom/0.1.0",
		}, nil
	})

	// "random" loader — returns random data useful for demos/testing
	r.Register("random", func(params map[string]string) (map[string]interface{}, error) {
		return map[string]interface{}{
			"number": rand.Intn(1000),
			"uuid":   generateSimpleID(),
			"color":  fmt.Sprintf("#%06x", rand.Intn(0xFFFFFF)),
		}, nil
	})

	// "json" loader — parses inline JSON data from a param
	r.Register("json", func(params map[string]string) (map[string]interface{}, error) {
		raw := params["data"]
		if raw == "" {
			return map[string]interface{}{}, nil
		}
		var result map[string]interface{}
		if err := json.Unmarshal([]byte(raw), &result); err != nil {
			return nil, fmt.Errorf("api/json: invalid JSON: %w", err)
		}
		return result, nil
	})
}

// =============================================================================
// Utility functions
// =============================================================================

func generateSimpleID() string {
	const chars = "abcdefghijklmnopqrstuvwxyz0123456789"
	b := make([]byte, 12)
	for i := range b {
		b[i] = chars[rand.Intn(len(chars))]
	}
	return string(b)
}

// FormatForScript converts a Go map to JS-compatible variable declarations
// that can be injected into a <script> block.
func FormatForScript(data map[string]interface{}) string {
	var sb strings.Builder
	for key, val := range data {
		jsVal := toJSLiteral(val)
		sb.WriteString(fmt.Sprintf("let %s = %s;\n", key, jsVal))
	}
	return sb.String()
}

func toJSLiteral(v interface{}) string {
	if v == nil {
		return "null"
	}
	switch val := v.(type) {
	case bool:
		if val {
			return "true"
		}
		return "false"
	case float64:
		if val == float64(int64(val)) {
			return fmt.Sprintf("%d", int64(val))
		}
		return fmt.Sprintf("%g", val)
	case int:
		return fmt.Sprintf("%d", val)
	case string:
		b, _ := json.Marshal(val)
		return string(b)
	case map[string]interface{}:
		var sb strings.Builder
		sb.WriteString("{ ")
		first := true
		for k, v2 := range val {
			if !first {
				sb.WriteString(", ")
			}
			sb.WriteString(fmt.Sprintf("%s: %s", k, toJSLiteral(v2)))
			first = false
		}
		sb.WriteString(" }")
		return sb.String()
	case []interface{}:
		var sb strings.Builder
		sb.WriteString("[")
		for i, v2 := range val {
			if i > 0 {
				sb.WriteString(", ")
			}
			sb.WriteString(toJSLiteral(v2))
		}
		sb.WriteString("]")
		return sb.String()
	default:
		b, err := json.Marshal(val)
		if err != nil {
			return "null"
		}
		return string(b)
	}
}
