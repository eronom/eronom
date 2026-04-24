package main

import (
	"bytes"
	"encoding/base64"
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

    // em-like HMR context
    window.__hmr_hooks = window.__hmr_hooks || { dispose: [], accept: [] };
    window.hmr = {
        data: window.__hmr_data || {},
        accept: function(cb) {
            window.__hmr_hooks.accept.push(cb);
        },
        dispose: function(cb) {
            window.__hmr_hooks.dispose.push(cb);
        },
        invalidate: function() {
            location.reload();
        }
    };
    // Ensure data persists properly
    window.__hmr_data = window.hmr.data;

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
        if (data.type === 'update' || data.type === 'hmr_erm') {
            let fetchPath = data.path || location.href;
            let url = new URL(fetchPath, location.origin);
            url.searchParams.set('t', new Date().getTime());
            
            console.log("[em] hot updated: " + (data.path || 'Document'));

            if (data.path && data.path.endsWith('.css')) {
                let links = document.querySelectorAll('link[rel="stylesheet"]');
                let updated = false;
                links.forEach(link => {
                    let linkUrl = new URL(link.href, location.origin);
                    if (linkUrl.pathname === data.path) {
                        link.href = url.href;
                        updated = true;
                    }
                });
                if (updated) {
                    return;
                }
            }

            // Call dispose handlers before DOM replacement
            window.__hmr_hooks.dispose.forEach(cb => {
                try { cb(window.hmr.data); } catch(e) { console.error("[HMR] Error in dispose handler", e); }
            });
            window.__hmr_hooks.dispose = [];
            window.__hmr_hooks.accept = []; // clear accept hooks for the next run

            // Clear prior intervals
            window.__hmr_intervals.forEach(clearInterval);
            window.__hmr_intervals = [];
            
            // Clear prior event listeners
            window.__hmr_listeners.forEach(({ target, type, listener, options }) => {
                target.removeEventListener(type, listener, options);
            });
            window.__hmr_listeners = [];

            fetch(url.href)
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
                             let sUrl = new URL(s.src, location.href);
                             sUrl.searchParams.set('t', new Date().getTime());
                             newScript.src = sUrl.href;
                        }
                        if (s.id) newScript.id = s.id;
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

func processErmConditions(content string) string {
	res := content
	count := 0
	for {
		start := strings.Index(res, "{#if ")
		if start == -1 {
			break
		}

		end := strings.Index(res[start:], "{/if}")
		if end == -1 {
			break
		}
		end += start

		block := res[start : end+5]

		scriptID := fmt.Sprintf("erm-cond-%d-%d", time.Now().UnixNano(), count)
		count++

		var branches []string
		rem := block[5:]

		closeBrace := strings.Index(rem, "}")
		if closeBrace == -1 {
			break
		}
		cond := strings.TrimSpace(rem[:closeBrace])
		rem = rem[closeBrace+1:]

		valid := true
		for {
			nextElseIf := strings.Index(rem, "{:else if ")
			nextElse := strings.Index(rem, "{:else}")
			nextEnd := strings.Index(rem, "{/if}")

			minIdx := nextEnd
			if minIdx == -1 {
				valid = false
				break
			}
			tokType := "end"

			if nextElseIf != -1 && nextElseIf < minIdx {
				minIdx = nextElseIf
				tokType = "elseif"
			}
			if nextElse != -1 && nextElse < minIdx {
				minIdx = nextElse
				tokType = "else"
			}

			htmlBody := rem[:minIdx]
			encodedHtml := base64.StdEncoding.EncodeToString([]byte(htmlBody))

			branches = append(branches, fmt.Sprintf(`if (%s) { htmlBase64 = "%s"; }`, cond, encodedHtml))

			if tokType == "end" {
				break
			} else if tokType == "else" {
				rem = rem[minIdx+7:]
				cond = "true"
			} else if tokType == "elseif" {
				rem = rem[minIdx+10:]
				clBr := strings.Index(rem, "}")
				if clBr == -1 {
					valid = false
					break
				}
				cond = strings.TrimSpace(rem[:clBr])
				rem = rem[clBr+1:]
			}
		}
		if !valid {
			break
		}

		js := fmt.Sprintf(`<script id="%s">
document.addEventListener('DOMContentLoaded', () => {
    let htmlBase64 = '';
    %s
    if (htmlBase64) {
        let me = document.getElementById("%s");
        if (me) {
            let html = decodeURIComponent(escape(atob(htmlBase64)));
            let tpl = document.createElement('template');
            tpl.innerHTML = html;
            me.parentNode.insertBefore(tpl.content, me.nextSibling);
        }
    }
}, { once: true });
</script>`, scriptID, strings.Join(branches, " else "), scriptID)

		res = res[:start] + js + res[end+5:]
	}
	return res
}

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

				if strings.HasSuffix(relPath, ".erm") || strings.HasSuffix(relPath, ".js") || strings.HasSuffix(relPath, ".css") || strings.HasSuffix(relPath, ".html") {
					path := "/" + strings.ReplaceAll(relPath, "\\", "/")
					msg := fmt.Sprintf(`{"type": "update", "path": "%s"}`, path)
					manager.broadcast([]byte(msg))
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

		if err == nil && !fileInfo.IsDir() {
			if strings.HasSuffix(fullPath, ".erm") {
				content, err := os.ReadFile(fullPath)
				if err == nil {
					// em-like API support
					content = bytes.ReplaceAll(content, []byte("import.meta.hot"), []byte("window.hmr"))

					processedContent := processErmConditions(string(content))
					content = []byte(processedContent)

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
			} else if strings.HasSuffix(fullPath, ".js") && reqPath != "/__hmr_client.js" {
				content, err := os.ReadFile(fullPath)
				if err == nil {
					// em-like API support for standalone js files
					content = bytes.ReplaceAll(content, []byte("import.meta.hot"), []byte("window.hmr"))
					w.Header().Set("Content-Type", "application/javascript")
					w.Header().Set("Cache-Control", "no-store, no-cache, must-revalidate, post-check=0, pre-check=0")
					w.Write(content)
					return
				}
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
