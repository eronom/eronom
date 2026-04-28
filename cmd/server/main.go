package main

import (
	"fmt"
	"log"
	"net/http"
	"os"
	"path/filepath"
)

func main() {
	dir := "."
	if len(os.Args) > 1 {
		dir = os.Args[1]
	}

	dir, err := filepath.Abs(dir)
	if err != nil {
		log.Fatal(err)
	}

	port := "8080"
	if envPort := os.Getenv("PORT"); envPort != "" {
		port = envPort
	}

	fmt.Printf("Production server running at http://localhost:%s\n", port)
	buildDir := filepath.Join(dir, "build")

	fs := http.FileServer(http.Dir(buildDir))

	http.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		path := r.URL.Path

		// Map root URL / to /index.html
		if path == "/" {
			path = "/index.html"
		} else if filepath.Ext(path) == "" {
			// If path has no extension, try to resolve it as a file or directory index
			if _, err := os.Stat(filepath.Join(buildDir, path+".html")); err == nil {
				path = path + ".html"
			} else if _, err := os.Stat(filepath.Join(buildDir, path, "index.html")); err == nil {
				path = path + "/index.html"
			}
		}

		// Prevent serving internal paths/directories accidentally if needed
		// Serve the resolved static file if it exists
		if stat, err := os.Stat(filepath.Join(buildDir, path)); err == nil && !stat.IsDir() {
			http.ServeFile(w, r, filepath.Join(buildDir, path))
			return
		}

		// Fallback to FileServer routing
		fs.ServeHTTP(w, r)
	})

	err = http.ListenAndServe(":"+port, nil)
	if err != nil {
		log.Fatal(err)
	}
}
