package main

import (
	"bytes"
	"fmt"
	"io/fs"
	"log"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/fsnotify/fsnotify"
	"github.com/gorilla/websocket"
)

var upgrader = websocket.Upgrader{
	CheckOrigin: func(r *http.Request) bool {
		return true
	},
}

type ClientManager struct {
	sync.RWMutex
	clients map[*websocket.Conn]bool
}

func (cm *ClientManager) broadcast(msg []byte) {
	cm.RLock()
	defer cm.RUnlock()
	for client := range cm.clients {
		client.WriteMessage(websocket.TextMessage, msg)
	}
}

var manager = &ClientManager{
	clients: make(map[*websocket.Conn]bool),
}

const hmrClientJS = `
(function() {
    if (window.__hmr_initialized) return;
    window.__hmr_initialized = true;

    window.__hmr_intervals = window.__hmr_intervals || [];
    const originalSetInterval = window.setInterval;
    window.setInterval = function(fn, t) {
        let id = originalSetInterval(fn, t);
        window.__hmr_intervals.push(id);
        return id;
    };

    window.__hmr_listeners = window.__hmr_listeners || [];
    const originalDocAddEventListener = document.addEventListener;
    document.addEventListener = function(type, listener, options) {
        window.__hmr_listeners.push({ target: document, type, listener, options });
        return originalDocAddEventListener.call(document, type, listener, options);
    };

    const originalWinAddEventListener = window.addEventListener;
    window.addEventListener = function(type, listener, options) {
        window.__hmr_listeners.push({ target: window, type, listener, options });
        return originalWinAddEventListener.call(window, type, listener, options);
    };

    let ws = new WebSocket("ws://" + location.host + "/__hmr");
    ws.onmessage = function(event) {
        let data = JSON.parse(event.data);
        if (data.type === 'hmr_erm') {
            console.log("[HMR] Module updated. Refreshing content seamlessly...");
            
            // Clear prior intervals
            window.__hmr_intervals.forEach(clearInterval);
            window.__hmr_intervals = [];
            
            // Clear prior event listeners
            window.__hmr_listeners.forEach(({ target, type, listener, options }) => {
                target.removeEventListener(type, listener, options);
            });
            window.__hmr_listeners = [];

            fetch(location.href)
                .then(r => r.text())
                .then(html => {
                    let parser = new DOMParser();
                    let doc = parser.parseFromString(html, 'text/html');
                    
                    document.title = doc.title;

                    let oldStyles = document.querySelectorAll('style, link[rel="stylesheet"]');
                    oldStyles.forEach(s => s.remove());
                    let newStyles = doc.querySelectorAll('style, link[rel="stylesheet"]');
                    newStyles.forEach(s => document.head.appendChild(s.cloneNode(true)));
                    
                    document.body.innerHTML = doc.body.innerHTML;

                    let scripts = document.body.querySelectorAll('script');
                    scripts.forEach(s => {
                        if(s.src && s.src.includes('__hmr_client.js')) return;
                        let newScript = document.createElement('script');
                        newScript.text = s.innerHTML;
                        if(s.src) {
                             let url = new URL(s.src, location.href);
                             url.searchParams.set('t', new Date().getTime());
                             newScript.src = url.href;
                        }
                        s.replaceWith(newScript);
                    });

                    document.dispatchEvent(new Event('DOMContentLoaded', {
                        bubbles: true,
                        cancelable: true
                    }));
                    window.dispatchEvent(new Event('load'));

                    console.log("[HMR] Hot Replacement complete.");
                });
        } else if (data.type === 'reload') {
            location.reload();
        }
    };
    ws.onclose = function() {
        console.log("[HMR] Connection closed. Trying to reconnect...");
        setInterval(function() { 
            let testWs = new WebSocket("ws://" + location.host + "/__hmr");
            testWs.onopen = function() { location.reload(); };
        }, 2000);
    };
    console.log("[HMR] Connected.");
})();
`

func watchFiles(dir string) {
	watcher, err := fsnotify.NewWatcher()
	if err != nil {
		log.Fatal(err)
	}

	err = filepath.WalkDir(dir, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.IsDir() {
			if strings.HasPrefix(d.Name(), ".") && d.Name() != "." {
				return filepath.SkipDir
			}
			return watcher.Add(path)
		}
		return nil
	})
	if err != nil {
		log.Fatal(err)
	}

	var mu sync.Mutex
	lastEvent := make(map[string]time.Time)

	for {
		select {
		case event, ok := <-watcher.Events:
			if !ok {
				return
			}
			if event.Op&fsnotify.Write == fsnotify.Write || event.Op&fsnotify.Create == fsnotify.Create {
				mu.Lock()
				last := lastEvent[event.Name]
				if time.Since(last) < 100*time.Millisecond {
					mu.Unlock()
					continue
				}
				lastEvent[event.Name] = time.Now()
				mu.Unlock()

				relPath, err := filepath.Rel(dir, event.Name)
				if err != nil {
					relPath = event.Name
				}
				relPath = filepath.ToSlash(relPath)

				fmt.Printf("File changed: %s\n", relPath)

				if strings.HasSuffix(relPath, ".erm") {
					manager.broadcast([]byte(`{"type": "hmr_erm"}`))
				}

				if event.Op&fsnotify.Create == fsnotify.Create {
					stat, err := os.Stat(event.Name)
					if err == nil && stat.IsDir() {
						watcher.Add(event.Name)
					}
				}
			}
		case err, ok := <-watcher.Errors:
			if !ok {
				return
			}
			log.Println("error:", err)
		}
	}
}

func main() {
	dir := "."
	if len(os.Args) > 1 {
		dir = os.Args[1]
	}

	dir, err := filepath.Abs(dir)
	if err != nil {
		log.Fatal(err)
	}

	go watchFiles(dir)

	port := "8080"

	http.HandleFunc("/__hmr", func(w http.ResponseWriter, r *http.Request) {
		conn, err := upgrader.Upgrade(w, r, nil)
		if err != nil {
			log.Println("upgrade:", err)
			return
		}

		manager.Lock()
		manager.clients[conn] = true
		manager.Unlock()

		conn.SetCloseHandler(func(code int, text string) error {
			manager.Lock()
			delete(manager.clients, conn)
			manager.Unlock()
			return nil
		})

		for {
			_, _, err := conn.ReadMessage()
			if err != nil {
				manager.Lock()
				delete(manager.clients, conn)
				manager.Unlock()
				break
			}
		}
	})

	http.HandleFunc("/__hmr_client.js", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/javascript")
		w.Header().Set("Cache-Control", "no-store")
		w.Write([]byte(hmrClientJS))
	})

	fs := http.FileServer(http.Dir(dir))
	http.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		reqPath := r.URL.Path
		if reqPath == "/" {
			reqPath = "/index.erm"
		}

		fullPath := filepath.Join(dir, reqPath)
		fileInfo, err := os.Stat(fullPath)

		if err == nil && !fileInfo.IsDir() && strings.HasSuffix(fullPath, ".erm") {
			content, err := os.ReadFile(fullPath)
			if err == nil {
				scriptTag := []byte(`<script src="/__hmr_client.js"></script>`)

				var injected []byte
				headIdx := bytes.Index(bytes.ToLower(content), []byte("</head>"))
				if headIdx != -1 {
					injected = append(injected, content[:headIdx]...)
					injected = append(injected, scriptTag...)
					injected = append(injected, content[headIdx:]...)
				} else {
					idx := bytes.LastIndex(bytes.ToLower(content), []byte("</body>"))
					if idx != -1 {
						injected = append(injected, content[:idx]...)
						injected = append(injected, scriptTag...)
						injected = append(injected, content[idx:]...)
					} else {
						injected = append(content, scriptTag...)
					}
				}

				w.Header().Set("Content-Type", "text/html; charset=utf-8")
				w.Header().Set("Cache-Control", "no-store, no-cache, must-revalidate, post-check=0, pre-check=0")
				w.Write(injected)
				return
			}
		}

		w.Header().Set("Cache-Control", "no-store, no-cache, must-revalidate, post-check=0, pre-check=0")
		fs.ServeHTTP(w, r)
	})

	fmt.Printf("Dev server running at http://localhost:%s\n", port)
	fmt.Printf("Serving files from: %s\n", dir)

	err = http.ListenAndServe(":"+port, nil)
	if err != nil {
		log.Fatal(err)
	}
}
