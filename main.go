package main

import (
	"bytes"
	"encoding/base64"
	"eronom/api"
	"eronom/eval"
	"eronom/route"
	"fmt"
	"hash/fnv"
	gohtml "html"
	"io/fs"
	"log"
	"net/http"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"sync"
	"time"

	"os/exec"

	"github.com/fsnotify/fsnotify"
	"github.com/gorilla/websocket"
)

// Global API router is now handled inside main() for a pure Fiber-style experience.

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

func parseReactivity(html string) (string, []string, []string) {
	var out strings.Builder
	var jsBindings []string
	var jsEvents []string

	inTag := false
	inString := false
	var stringChar byte

	i := 0
	for i < len(html) {
		c := html[i]

		if !inTag {
			if c == '<' {
				inTag = true
				out.WriteByte(c)
				i++
				continue
			}
			if c == '{' && i+1 < len(html) && html[i+1] != '#' && html[i+1] != ':' && html[i+1] != '/' {
				start := i
				depth := 1
				curr := i + 1
				for curr < len(html) && depth > 0 {
					switch html[curr] {
					case '{':
						depth++
					case '}':
						depth--
					}
					curr++
				}
				if depth == 0 {
					expr := html[start+1 : curr-1]
					id := fmt.Sprintf("erm-bind-%d-%d", time.Now().UnixNano(), i)

					// Inject a placeholder that we will replace during SSR with the escaped initial value
					exprB64 := base64.StdEncoding.EncodeToString([]byte(expr))
					out.WriteString(fmt.Sprintf(`<span id="%s"><!--erm-expr:%s--></span>`, id, exprB64))

					jsBindings = append(jsBindings, fmt.Sprintf(`window.__erm_bindings.push({ id: "%s", get: () => (%s) });`, id, expr))
					i = curr
					continue
				}
			}
		} else {
			if c == '>' && !inString {
				inTag = false
				out.WriteByte(c)
				i++
				continue
			}
			if (c == '"' || c == '\'') && !inString {
				inString = true
				stringChar = c
				out.WriteByte(c)
				i++
				continue
			}
			if c == stringChar && inString {
				inString = false
				out.WriteByte(c)
				i++
				continue
			}

			if !inString && i > 0 && (html[i-1] == ' ' || html[i-1] == '\t' || html[i-1] == '\n') && strings.HasPrefix(html[i:], "on") {
				eqIdx := strings.Index(html[i:], "=")
				if eqIdx > 2 && eqIdx < 30 {
					attrName := html[i : i+eqIdx]
					if !strings.ContainsAny(attrName, " \t\n>") {
						if i+eqIdx+1 < len(html) && html[i+eqIdx+1] == '{' {
							start := i + eqIdx + 1
							depth := 1
							curr := start + 1
							for curr < len(html) && depth > 0 {
								if html[curr] == '{' {
									depth++
								}
								if html[curr] == '}' {
									depth--
								}
								curr++
							}
							if depth == 0 {
								expr := html[start+1 : curr-1]
								eventName := strings.ToLower(attrName[2:])
								exprB64 := base64.StdEncoding.EncodeToString([]byte(expr))

								out.WriteString(fmt.Sprintf(`data-erm-evt-%s="<!--erm-evt:%s-->"`, eventName, exprB64))

								i = curr
								continue
							}
						}
					}
				}
			}

			// Support bind:value={variable}
			if !inString && i > 0 && (html[i-1] == ' ' || html[i-1] == '\t' || html[i-1] == '\n') && strings.HasPrefix(html[i:], "bind:value={") {
				start := i + len("bind:value={") - 1
				depth := 1
				curr := start + 1
				for curr < len(html) && depth > 0 {
					if html[curr] == '{' {
						depth++
					}
					if html[curr] == '}' {
						depth--
					}
					curr++
				}
				if depth == 0 {
					expr := html[start+1 : curr-1]
					id := fmt.Sprintf("erm-bind-val-%d-%d", time.Now().UnixNano(), i)
					out.WriteString(fmt.Sprintf(`id="%s" data-erm-evt-input-bind="%s"`, id, id))

					// Update variable on input
					jsEvents = append(jsEvents, fmt.Sprintf(`window.__erm_events.push({ id: "%s", event: "input", handler: function(event) { %s = event.target.value; if (typeof window.__erm_update === 'function') window.__erm_update(); } });`, id, expr))
					// Update input when variable changes
					jsBindings = append(jsBindings, fmt.Sprintf(`window.__erm_bindings.push({ id: "%s", type: "value", get: () => (%s) });`, id, expr))

					i = curr
					continue
				}
			}
		}

		out.WriteByte(c)
		i++
	}
	return out.String(), jsBindings, jsEvents
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

	html, jsBindings, jsEvents := parseReactivity(html)

	reactivityScript := strings.Join(jsBindings, "\n") + "\n" + strings.Join(jsEvents, "\n")
	if strings.TrimSpace(reactivityScript) != "" {
		nodeScripts = append(nodeScripts, reactivityScript)
	}

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

	// Evaluate scripts on server for SSR using lightweight evaluator
	scriptSource := strings.Join(userScripts, "\n")
	ev := eval.NewErmEval()
	ev.ParseScriptVars(scriptSource)

	// 1.5 Process bindings for true SSR with XSS protection (escape_html)
	reExpr := regexp.MustCompile(`<!--erm-expr:([a-zA-Z0-9+/=]+)-->`)

	// 1.8 Process {#for} logic
	var generatedLogic []string
	reFor := regexp.MustCompile(`(?s)\{#for\s+([a-zA-Z_$][a-zA-Z0-9_$]*)(?:\s*,\s*([a-zA-Z_$][a-zA-Z0-9_$]*))?\s+in\s+([^}]+)\}(.*?)\{/for\}`)
	for {
		match := reFor.FindStringSubmatch(res)
		if match == nil {
			break
		}

		fullMatch := match[0]
		itemName := strings.TrimSpace(match[1])
		indexName := strings.TrimSpace(match[2])
		collectionExpr := strings.TrimSpace(match[3])
		body := match[4]

		// For loop template should not have erm-bind spans for internal reactivity,
		// because we re-render the whole loop anyway.
		reSpanUnwrap := regexp.MustCompile(`<span id="erm-bind-[^"]+">(<!--erm-expr:[a-zA-Z0-9+/=]+-->)</span>`)
		templateBody := reSpanUnwrap.ReplaceAllString(body, "$1")

		anchorID := fmt.Sprintf("erm-for-anchor-%d", time.Now().UnixNano())

		// SSR for for-loop
		var ssrHtml strings.Builder
		itemsVal, evalErr := ev.Eval(collectionExpr)
		if evalErr != nil {
			fmt.Printf("[SSR] Error evaluating loop collection '%s': %v\n", collectionExpr, evalErr)
		}

		if items, ok := itemsVal.([]interface{}); ok {
			for i, item := range items {
				subEv := ev.Clone()
				subEv.Set(itemName, item)
				if indexName != "" {
					subEv.Set(indexName, float64(i))
				}

				// Replace expressions in body for this iteration
				iterBody := reExpr.ReplaceAllStringFunc(templateBody, func(match string) string {
					parts := reExpr.FindStringSubmatch(match)
					b64 := parts[1]
					exprBytes, _ := base64.StdEncoding.DecodeString(b64)
					expr := string(exprBytes)
					val, evalErr := subEv.Eval(expr)
					if evalErr == nil && val != nil {
						return gohtml.EscapeString(fmt.Sprintf("%v", val))
					}
					return ""
				})
				ssrHtml.WriteString(iterBody)
			}
		} else {
			if itemsVal != nil {
				fmt.Printf("[SSR] Loop collection '%s' is not an array: %T\n", collectionExpr, itemsVal)
			} else {
				fmt.Printf("[SSR] Loop collection '%s' not found or nil\n", collectionExpr)
			}
		}

		bodyB64 := base64.StdEncoding.EncodeToString([]byte(templateBody))

		anchorHTML := fmt.Sprintf(`<span id="%s" style="display:contents;">%s</span>`, anchorID, ssrHtml.String())

		jsForParams := itemName
		if indexName != "" {
			jsForParams = itemName + ", " + indexName
		}

		logic := fmt.Sprintf(`
	{
		let __erm_anchor = document.getElementById("%s");
		if (__erm_anchor) {
			let __erm_items = [];
			try { __erm_items = (%s); } catch(e) {}
			if (!Array.isArray(__erm_items)) __erm_items = [];
			
			let __erm_itemsJson = JSON.stringify(__erm_items);
			if (__erm_anchor.__erm_last_items !== __erm_itemsJson) {
				__erm_anchor.__erm_last_items = __erm_itemsJson;
				let __erm_template = decodeURIComponent(escape(atob("%s")));
				let __erm_html = "";
				__erm_items.forEach((%s) => {
					let __erm_iter_html = __erm_template.replace(/<!--erm-expr:([a-zA-Z0-9+/=]+)-->/g, (m, b64) => {
						try {
							return eval(decodeURIComponent(escape(atob(b64))));
						} catch(e) { return ""; }
					});
					// Handle events by baking loop variables into a closure
					__erm_iter_html = __erm_iter_html.replace(/data-erm-evt-([a-z-]+)="<!--erm-evt:([a-zA-Z0-9+/=]+)-->"/g, (m, eventName, b64) => {
						let expr = atob(b64);
						let baked = "((event) => { " + 
							"let %[5]s = " + JSON.stringify(%[5]s) + "; " +
							("%[6]s" ? "let %[6]s = " + JSON.stringify(%[6]s) + "; " : "") +
							"let __erm_fn = (" + expr + "); " +
							"if (typeof __erm_fn === 'function') __erm_fn(event); " +
							"})";
						return 'data-erm-evt-' + eventName + '="' + btoa(baked) + '"';
					});
					// Handle boolean attributes (checked, disabled, etc.) and remove if value is falsy
					__erm_iter_html = __erm_iter_html.replace(/([a-z-]+)="false"/g, "");
					__erm_iter_html = __erm_iter_html.replace(/([a-z-]+)="true"/g, "$1");
					__erm_html += __erm_iter_html;
				});
				__erm_anchor.innerHTML = __erm_html;
			}
		}
	}`, anchorID, collectionExpr, bodyB64, jsForParams, itemName, indexName)

		generatedLogic = append(generatedLogic, logic)
		res = strings.Replace(res, fullMatch, anchorHTML, 1)
	}

	res = reExpr.ReplaceAllStringFunc(res, func(match string) string {
		parts := reExpr.FindStringSubmatch(match)
		b64 := parts[1]
		exprBytes, decodeErr := base64.StdEncoding.DecodeString(b64)
		if decodeErr != nil {
			return ""
		}
		expr := string(exprBytes)
		val, evalErr := ev.Eval(expr)
		if evalErr == nil && val != nil {
			// Svelte-like true SSR: escape HTML output to prevent XSS
			return gohtml.EscapeString(fmt.Sprintf("%v", val))
		}
		return ""
	})

	// 2. Process {#if} logic and replace with hidden anchor spans
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

		initialHTML := ""
		initialBase64 := ""
		ssrMatched := false

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

			branches = append(branches, fmt.Sprintf(`if (%s) { __erm_newHtmlBase64 = "%s"; }`, cond, encodedHtml))

			if !ssrMatched {
				if condVal, condErr := ev.EvalBool(cond); condErr == nil && condVal {
					initialHTML = htmlBody
					initialBase64 = encodedHtml
					ssrMatched = true
				}
			}

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

		anchorHTML := fmt.Sprintf(`<span id="%s" data-ssr-base64="%s" style="display:contents;">%s</span>`, anchorID, initialBase64, initialHTML)

		logic := fmt.Sprintf(`
	{
		let __erm_me = document.getElementById("%s");
		if (__erm_me) {
			if (typeof __erm_me.__erm_cond_last === 'undefined') {
				__erm_me.__erm_cond_last = __erm_me.getAttribute('data-ssr-base64') || '';
			}
			let __erm_newHtmlBase64 = '';
			%s
			if (__erm_me.__erm_cond_last !== __erm_newHtmlBase64) {
				__erm_me.__erm_cond_last = __erm_newHtmlBase64;
				if (__erm_newHtmlBase64 === '') {
					__erm_me.innerHTML = '';
				} else {
					__erm_me.innerHTML = decodeURIComponent(escape(atob(__erm_newHtmlBase64)));
				}
			}
		}
	}`, anchorID, strings.Join(branches, " else "))

		generatedLogic = append(generatedLogic, logic)
		res = res[:start] + anchorHTML + res[end+5:]
	}

	// 3. Assemble final compiled JS block
	finalJS := fmt.Sprintf(`<script>
window.signal = window.signal || function(val) { return val; };
(() => {
	window.__erm_bindings = window.__erm_bindings || [];
	window.__erm_events = window.__erm_events || [];

	window.__erm_update = function() {
		%s
		window.__erm_bindings.forEach(b => {
			try {
				let val = b.get();
				if (b.last !== val) {
					b.last = val;
					let el = document.getElementById(b.id);
					if (el) {
						if (b.type === 'value') {
							if (el.value !== val) el.value = val;
						} else {
							el.innerText = val;
						}
					}
				}
			} catch(e) {}
		});
	};

	// User Logic
	%s

	// Conditional Engine
	const __erm_handle_event = (e) => {
		// 1. Dynamic expressions (from loops, etc.)
		let target = e.target.closest('[data-erm-evt-' + e.type + ']');
		if (target) {
			let raw = target.getAttribute('data-erm-evt-' + e.type);
			if (raw) {
				try {
					let expr = raw;
					if (raw.startsWith('<!--erm-evt:')) {
						expr = atob(raw.slice(12, -3));
					} else {
						expr = atob(raw);
					}
					const result = eval(expr);
					if (typeof result === 'function') result(e);
					if (typeof window.__erm_update === 'function') window.__erm_update();
				} catch(err) { console.error(err); }
			}
		}
		// 2. Static events (bind:value, etc.)
		window.__erm_events.forEach(ev => {
			if (ev.event === e.type) {
				let staticTarget = e.target.closest('[data-erm-evt-' + ev.event + '-bind="' + ev.id + '"]');
				if (staticTarget) {
					ev.handler(e);
				}
			}
		});
	};

	document.addEventListener('DOMContentLoaded', () => {
		['click', 'input', 'change', 'keydown', 'keyup', 'submit'].forEach(type => {
			document.addEventListener(type, __erm_handle_event);
		});
		window.__erm_update();
	}, { once: true });
})();
</script>`, strings.Join(generatedLogic, "\n"), strings.Join(userScripts, "\n"))

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

func buildProject(sourceDir, outDir string) error {
	fmt.Println("Building project to", outDir)
	os.RemoveAll(outDir)
	err := os.MkdirAll(outDir, 0755)
	if err != nil {
		return err
	}

	layouts := make(map[string]string)
	err = filepath.Walk(sourceDir, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}
		if info.IsDir() {
			if path != sourceDir && (info.Name() == "build" || strings.HasPrefix(info.Name(), ".")) {
				return filepath.SkipDir
			}
			return nil
		}
		if info.Name() == "layout.erm" {
			b, _ := os.ReadFile(path)
			layouts[filepath.Dir(path)] = string(b)
		}
		return nil
	})
	if err != nil {
		return err
	}

	err = filepath.Walk(sourceDir, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}
		if strings.HasPrefix(info.Name(), ".") && info.Name() != "." {
			if info.IsDir() {
				return filepath.SkipDir
			}
			return nil
		}

		if info.IsDir() {
			if path != sourceDir && (info.Name() == "build" || info.Name() == "tmp") {
				return filepath.SkipDir
			}
			return nil
		}

		// Skip Go source files and the binary itself
		if strings.HasSuffix(info.Name(), ".go") || info.Name() == "go.mod" || info.Name() == "go.sum" || info.Name() == "eronom" {
			return nil
		}

		relPath, _ := filepath.Rel(sourceDir, path)
		outPath := filepath.Join(outDir, relPath)

		if strings.HasSuffix(info.Name(), ".erm") {
			if info.Name() == "layout.erm" || info.Name() == "virtual_root.erm" {
				return nil
			}
			// Skip components (Uppercase .erm)
			if len(info.Name()) > 0 && info.Name()[0] >= 'A' && info.Name()[0] <= 'Z' {
				return nil
			}

			contentBytes, _ := os.ReadFile(path)
			content := string(contentBytes)
			content = strings.ReplaceAll(content, "import.meta.hot", "window.hmr")

			currentDir := filepath.Dir(path)
			var layoutContent string
			for {
				if l, ok := layouts[currentDir]; ok {
					layoutContent = l
					break
				}
				if currentDir == sourceDir || currentDir == filepath.Dir(currentDir) {
					break
				}
				currentDir = filepath.Dir(currentDir)
			}

			if layoutContent != "" {
				originalPage := content
				content = strings.ReplaceAll(layoutContent, "<slot />", originalPage)
				content = strings.ReplaceAll(content, "<slot></slot>", originalPage)
			}

			processedContent := processErmComponent(filepath.Dir(path), content)

			ext := filepath.Ext(outPath)
			base := outPath[:len(outPath)-len(ext)]
			baseName := filepath.Base(base)
			dirName := filepath.Dir(base)

			if baseName == "page" || baseName == "index" {
				outPath = filepath.Join(dirName, "index.html")
			} else {
				outPath = filepath.Join(dirName, baseName, "index.html")
			}

			os.MkdirAll(filepath.Dir(outPath), 0755)
			return os.WriteFile(outPath, []byte(processedContent), 0644)

		} else {
			os.MkdirAll(filepath.Dir(outPath), 0755)
			b, err := os.ReadFile(path)
			if err != nil {
				return err
			}
			return os.WriteFile(outPath, b, 0644)
		}
	})

	// Compile the Go binary for production
	fmt.Println("Compiling Go binary for production...")
	buildCmd := exec.Command("go", "build", "-o", filepath.Join(outDir, "eronom"), "main.go")
	buildCmd.Stdout = os.Stdout
	buildCmd.Stderr = os.Stderr
	if err := buildCmd.Run(); err != nil {
		return fmt.Errorf("failed to compile binary: %v", err)
	}

	return err
}

func initProject(dir string) error {
	fmt.Println("Initializing fresh Eronom project in", dir)
	err := os.MkdirAll(dir, 0755)
	if err != nil {
		return err
	}

	indexContent := `<script>
	let name = "Eronom";
</script>

<style>
	main {
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		height: 100vh;
		font-family: 'Inter', sans-serif;
		background: #0f172a;
		color: white;
	}
	h1 {
		font-size: 3rem;
		margin-bottom: 0.5rem;
		background: linear-gradient(to right, #38bdf8, #818cf8);
		-webkit-background-clip: text;
		-webkit-text-fill-color: transparent;
	}
	p { color: #94a3b8; }
</style>

<main>
	<h1>Hello {name}!</h1>
	<p>Your ultra-fast SSR project is ready.</p>
</main>
`
	layoutContent := `<!DOCTYPE html>
<html lang="en">
<head>
	<meta charset="UTF-8">
	<meta name="viewport" content="width=device-width, initial-scale=1.0">
	<title>Eronom App</title>
	<link rel="preconnect" href="https://fonts.googleapis.com">
	<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
	<link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;700&display=swap" rel="stylesheet">
</head>
<body>
	<slot />
</body>
</html>
`

	err = os.WriteFile(filepath.Join(dir, "index.erm"), []byte(indexContent), 0644)
	if err != nil {
		return err
	}

	err = os.WriteFile(filepath.Join(dir, "layout.erm"), []byte(layoutContent), 0644)
	if err != nil {
		return err
	}

	fmt.Println("Success! Run './eronom' to start development.")
	return nil
}

func main() {
	app := route.NewApp()

	// Register API routes from the api package
	api.Routes(app)

	cmd := "dev"
	dir := "."
	if len(os.Args) > 1 {
		if os.Args[1] == "build" || os.Args[1] == "dev" || os.Args[1] == "start" || os.Args[1] == "init" {
			cmd = os.Args[1]
			if len(os.Args) > 2 {
				dir = os.Args[2]
			}
		} else {
			dir = os.Args[1]
		}
	}

	dir, err := filepath.Abs(dir)
	if err != nil {
		log.Fatal(err)
	}

	if cmd == "build" {
		err := buildProject(dir, filepath.Join(dir, "build"))
		if err != nil {
			log.Fatal(err)
		}
		fmt.Println("Build successful! Ready for production deployment.")
		return
	}

	if cmd == "init" {
		err := initProject(dir)
		if err != nil {
			log.Fatal(err)
		}
		return
	}

	port := "8080"

	if cmd == "start" {
		fmt.Printf("Production server running at http://localhost:%s\n", port)
		buildDir := filepath.Join(dir, "build")

		// If we are already inside the build directory, use it directly
		if _, err := os.Stat(buildDir); os.IsNotExist(err) {
			buildDir = dir
		}

		fmt.Printf("Serving production assets from: %s\n", buildDir)

		fs := http.FileServer(http.Dir(buildDir))
		http.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
			path := r.URL.Path
			if path == "/" {
				path = "/index.html"
			} else if filepath.Ext(path) == "" {
				if _, err := os.Stat(filepath.Join(buildDir, path+".html")); err == nil {
					path = path + ".html"
				} else if _, err := os.Stat(filepath.Join(buildDir, path, "index.html")); err == nil {
					path = path + "/index.html"
				}
			}

			if _, err := os.Stat(filepath.Join(buildDir, path)); err == nil {
				http.ServeFile(w, r, filepath.Join(buildDir, path))
				return
			}

			fs.ServeHTTP(w, r)
		})

		err = http.ListenAndServe(":"+port, nil)
		if err != nil {
			log.Fatal(err)
		}
		return
	}

	go watchFiles(dir)

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

	// Mount API routes at /api/* — Next.js-style API endpoints
	http.HandleFunc("/api/", func(w http.ResponseWriter, r *http.Request) {
		app.ServeHTTP(w, r)
	})

	fs := http.FileServer(http.Dir(dir))
	http.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		reqPath := r.URL.Path

		// Prevent direct URL access to .erm files
		if strings.HasSuffix(reqPath, ".erm") {
			http.NotFound(w, r)
			return
		}

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
