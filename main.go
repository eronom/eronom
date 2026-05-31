package main

import (
	"fmt"
	"log"
	"net/http"

	"eronom/todo"
)

// Log requests
func logger(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		log.Printf("%s %s %s", r.RemoteAddr, r.Method, r.URL.Path)
		next.ServeHTTP(w, r)
	})
}

func main() {
	mux := http.NewServeMux()

	mux.HandleFunc("GET /todos", todo.HandleGetTodos)
	mux.HandleFunc("POST /todos", todo.HandleCreateTodo)
	mux.HandleFunc("PUT /todos/{id}", todo.HandleUpdateTodo)
	mux.HandleFunc("DELETE /todos/{id}", todo.HandleDeleteTodo)

	// Add a simple welcome route
	mux.HandleFunc("GET /", func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/" {
			http.NotFound(w, r)
			return
		}
		w.Header().Set("Content-Type", "text/plain")
		fmt.Fprintf(w, "Welcome to the Go Todo API! Use GET/POST /todos, or PUT/DELETE /todos/{id}\n")
	})

	port := ":8080"
	log.Printf("Starting Todo API server on http://localhost%s", port)
	if err := http.ListenAndServe(port, logger(mux)); err != nil {
		log.Fatal(err)
	}
}
