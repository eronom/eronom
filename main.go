package main

import (
	"bytes"
	"encoding/base64"
	"fmt"
	"hash/fnv"
	"io/fs"
	"log"
	"net/http"
	"os"
	"path/filepath"
	"regexp"
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
            
            // If it's a template/HTML file, always fetch the current page so components re-render within their parent scope.
            if (data.path && (data.path.endsWith('.erm') || data.path.endsWith('.html'))) {
                fetchPath = location.href;
            }

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

func scopeCSS(css, scopeID string) string {
	re := regexp.MustCompile(`(?s)([^\{\}]+)\{([^\{\}]*)\}`)
	return re.ReplaceAllStringFunc(css, func(match string) string {
		parts := re.FindStringSubmatch(match)

		rawSelector := parts[1]
		block := parts[2]

		trimmedSelector := strings.TrimSpace(rawSelector)
		if trimmedSelector == "" {
			return match
		}
		idx := strings.Index(rawSelector, trimmedSelector)
		leadingWhite := rawSelector[:idx]

		sels := strings.Split(trimmedSelector, ",")
		for i, s := range sels {
			s = strings.TrimSpace(s)
			if s != "" {
				if strings.Contains(s, "%") || s == "to" || s == "from" || strings.HasPrefix(s, "body") || strings.HasPrefix(s, "html") {
					continue
				}

				semiIdx := strings.Index(s, ":")
				if semiIdx != -1 {
					sels[i] = s[:semiIdx] + "[" + scopeID + "]" + s[semiIdx:]
				} else {
					sels[i] = s + "[" + scopeID + "]"
				}
			}
		}
		return leadingWhite + strings.Join(sels, ", ") + " {" + block + "}"
	})
}

func scopeHTML(html, scopeID string) string {
	reTag := regexp.MustCompile(`(?i)<([a-zA-Z0-9-:]+)([^>]*)>`)
	return reTag.ReplaceAllStringFunc(html, func(match string) string {
		parts := reTag.FindStringSubmatch(match)
		tag := parts[1]
		attrs := parts[2]

		if len(tag) > 0 && tag[0] >= 'A' && tag[0] <= 'Z' {
			return match
		}
		if strings.ToLower(tag) == "html" || strings.ToLower(tag) == "head" || strings.ToLower(tag) == "body" || tag == "!DOCTYPE" || strings.ToLower(tag) == "script" || strings.ToLower(tag) == "style" {
			return match
		}

		return "<" + tag + " " + scopeID + attrs + ">"
	})
}

func processComponentTree(baseDir, content string, visited map[string]bool) (string, []string, []string) {
	var nodeScripts []string
	var nodeStyles []string

	h := fnv.New32a()
	h.Write([]byte(content))
	scopeID := fmt.Sprintf("data-e-%x", h.Sum32())

	reScript := regexp.MustCompile(`(?s)(<script(?:\s+[^>]*?)?>)(.*?)(</script>)`)
	html := reScript.ReplaceAllStringFunc(content, func(match string) string {
		parts := reScript.FindStringSubmatch(match)
		if strings.Contains(parts[1], "src=") {
			return match
		}
		nodeScripts = append(nodeScripts, strings.TrimSpace(parts[2]))
		return ""
	})

	reStyle := regexp.MustCompile(`(?s)<style[^>]*>(.*?)</style>`)
	html = reStyle.ReplaceAllStringFunc(html, func(match string) string {
		parts := reStyle.FindStringSubmatch(match)
		cssContent := parts[1]
		scopedCssContent := scopeCSS(cssContent, scopeID)
		nodeStyles = append(nodeStyles, scopedCssContent)
		return ""
	})

	html = scopeHTML(html, scopeID)

	reImport := regexp.MustCompile(`(?m)^[\s]*import\s+([A-Z][a-zA-Z0-9]*)\s+from\s+['"](.+?\.erm)['"]\s*;?[\s]*$`)
	imports := make(map[string]string)
	for i, script := range nodeScripts {
		nodeScripts[i] = reImport.ReplaceAllStringFunc(script, func(match string) string {
			parts := reImport.FindStringSubmatch(match)
			imports[parts[1]] = parts[2]
			return ""
		})
	}

	reCompTag := regexp.MustCompile(`(?s)<([A-Z][a-zA-Z0-9]*)\s*/>`)
	html = reCompTag.ReplaceAllStringFunc(html, func(match string) string {
		parts := reCompTag.FindStringSubmatch(match)
		compName := parts[1]

		compRelPath, ok := imports[compName]
		if !ok {
			compRelPath = compName + ".erm"
		}

		compPath := filepath.Join(baseDir, compRelPath)
		absPath, _ := filepath.Abs(compPath)
		if visited[absPath] {
			return match
		}
		visitedChild := make(map[string]bool)
		for k, v := range visited {
			visitedChild[k] = v
		}
		visitedChild[absPath] = true

		compContentBytes, err := os.ReadFile(compPath)
		if err != nil {
			return match
		}

		compHtml, compScripts, compStyles := processComponentTree(filepath.Dir(compPath), string(compContentBytes), visitedChild)
		for _, s := range compScripts {
			if strings.TrimSpace(s) != "" {
				nodeScripts = append(nodeScripts, fmt.Sprintf("{\n%s\n}", s))
			}
		}
		for _, s := range compStyles {
			if strings.TrimSpace(s) != "" {
				nodeStyles = append(nodeStyles, s)
			}
		}

		return compHtml
	})

	return html, nodeScripts, nodeStyles
}

func processErmComponent(baseDir string, content string) string {
	absBase, _ := filepath.Abs(baseDir)
	visited := map[string]bool{filepath.Join(absBase, "virtual_root.erm"): true}
	res, userScripts, userStyles := processComponentTree(baseDir, content, visited)

	// 2. Process {#if} logic and replace with hidden anchor spans
	var generatedLogic []string
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

		anchorID := fmt.Sprintf("erm-cond-anchor-%d-%d", time.Now().UnixNano(), count)
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

		anchorHTML := fmt.Sprintf(`<span id="%s" style="display:none;"></span>`, anchorID)

		logic := fmt.Sprintf(`
	{
		let htmlBase64 = '';
		%s
		if (htmlBase64) {
			let me = document.getElementById("%s");
			if (me) {
				let html = decodeURIComponent(escape(atob(htmlBase64)));
				let tpl = document.createElement('template');
				tpl.innerHTML = html;
				me.parentNode.insertBefore(tpl.content, me);
			}
		}
	}`, strings.Join(branches, " else "), anchorID)

		generatedLogic = append(generatedLogic, logic)
		res = res[:start] + anchorHTML + res[end+5:]
	}

	// 3. Assemble final compiled JS block
	finalJS := fmt.Sprintf(`<script>
(() => {
	// User Logic
	%s

	// Conditional Engine
	document.addEventListener('DOMContentLoaded', () => {
		%s
	}, { once: true });
})();
</script>`, strings.Join(userScripts, "\n"), strings.Join(generatedLogic, "\n"))

	finalCSS := ""
	if len(userStyles) > 0 {
		finalCSS = fmt.Sprintf("\n<style>\n%s\n</style>\n", strings.Join(userStyles, "\n"))
	}

	return res + finalCSS + finalJS
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
		var fullPath string
		var fileInfo os.FileInfo
		var err error

		// Next.js-like file-based routing resolution
		if reqPath == "/" {
			candidates := []string{"/page.erm", "/index.erm"}
			for _, c := range candidates {
				candidatePath := filepath.Join(dir, c)
				info, e := os.Stat(candidatePath)
				if e == nil && !info.IsDir() {
					fullPath = candidatePath
					fileInfo = info
					err = nil
					break
				}
			}
			if fullPath == "" {
				fullPath = filepath.Join(dir, "/index.erm")
				fileInfo, err = os.Stat(fullPath)
			}
		} else if filepath.Ext(reqPath) == "" {
			// No extension, try app/pages routing patterns
			candidates := []string{
				reqPath + "/page.erm",
				reqPath + ".erm",
				reqPath + "/index.erm",
			}
			for _, c := range candidates {
				candidatePath := filepath.Join(dir, c)
				info, e := os.Stat(candidatePath)
				if e == nil && !info.IsDir() {
					fullPath = candidatePath
					fileInfo = info
					err = nil
					break
				}
			}
			if fullPath == "" {
				fullPath = filepath.Join(dir, reqPath)
				fileInfo, err = os.Stat(fullPath)
			}
		} else {
			fullPath = filepath.Join(dir, reqPath)
			fileInfo, err = os.Stat(fullPath)
		}

		if err == nil && !fileInfo.IsDir() {
			if strings.HasSuffix(fullPath, ".erm") {
				content, err := os.ReadFile(fullPath)
				if err == nil {
					// em-like API support
					content = bytes.ReplaceAll(content, []byte("import.meta.hot"), []byte("window.hmr"))
					pageContent := string(content)

					// Basic Next.js-like layout support
					currentDir := filepath.Dir(fullPath)
					var layoutBytes []byte
					for {
						layoutPath := filepath.Join(currentDir, "layout.erm")
						if layoutInfo, err := os.Stat(layoutPath); err == nil && !layoutInfo.IsDir() && fullPath != layoutPath {
							layoutBytes, _ = os.ReadFile(layoutPath)
							break
						}
						if currentDir == dir || currentDir == filepath.Dir(currentDir) {
							break
						}
						currentDir = filepath.Dir(currentDir)
					}

					if len(layoutBytes) > 0 {
						layoutContent := string(layoutBytes)
						originalPage := pageContent
						pageContent = strings.ReplaceAll(layoutContent, "<slot />", originalPage)
						pageContent = strings.ReplaceAll(pageContent, "<slot></slot>", originalPage)
					}

					processedContent := processErmComponent(filepath.Dir(fullPath), pageContent)
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
