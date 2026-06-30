(function() {
  if (window.__hmr_initialized) return;
  window.__hmr_initialized = true;
  console.log("[HMR] Initialized");

  const OVERLAY_CSS = `
#erm-error-overlay {
  position: fixed;
  top: 0;
  left: 0;
  width: 100vw;
  height: 100vh;
  background: rgba(10, 10, 12, 0.85);
  backdrop-filter: blur(8px);
  -webkit-backdrop-filter: blur(8px);
  z-index: 999999;
  display: flex;
  justify-content: center;
  align-items: center;
  color: #f3f4f6;
  font-family: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
  animation: erm-fade-in 0.25s ease-out;
}

@keyframes erm-fade-in {
  from { opacity: 0; }
  to { opacity: 1; }
}

.erm-error-card {
  background: #18181b;
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 16px;
  width: 90%;
  max-width: 800px;
  max-height: 85vh;
  box-shadow: 0 20px 50px rgba(0, 0, 0, 0.6);
  display: flex;
  flex-direction: column;
  overflow: hidden;
  animation: erm-slide-up 0.3s cubic-bezier(0.16, 1, 0.3, 1);
}

@keyframes erm-slide-up {
  from { transform: translateY(20px) scale(0.98); }
  to { transform: translateY(0) scale(1); }
}

.erm-error-header {
  padding: 20px 24px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.08);
  display: flex;
  align-items: center;
  justify-content: space-between;
  background: #202024;
}

.erm-error-badge {
  background: #ef4444;
  color: #ffffff;
  font-size: 11px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  padding: 4px 10px;
  border-radius: 9999px;
  box-shadow: 0 0 10px rgba(239, 68, 68, 0.3);
}

.erm-error-file {
  color: #a1a1aa;
  font-family: "SFMono-Regular", Consolas, "Liberation Mono", Menlo, monospace;
  font-size: 13px;
  margin-left: 12px;
  flex-grow: 1;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.erm-error-close-btn {
  background: transparent;
  border: none;
  color: #71717a;
  cursor: pointer;
  padding: 4px;
  border-radius: 4px;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: all 0.2s;
}

.erm-error-close-btn:hover {
  color: #f4f4f5;
  background: rgba(255, 255, 255, 0.05);
}

.erm-error-body {
  padding: 24px;
  overflow-y: auto;
  flex-grow: 1;
}

.erm-error-title {
  font-size: 18px;
  font-weight: 600;
  margin-top: 0;
  margin-bottom: 16px;
  color: #ffffff;
  line-height: 1.4;
}

.erm-error-msg {
  background: #09090b;
  border-left: 4px solid #ef4444;
  padding: 16px;
  border-radius: 0 8px 8px 0;
  font-family: "SFMono-Regular", Consolas, "Liberation Mono", Menlo, monospace;
  font-size: 13.5px;
  line-height: 1.6;
  color: #f3f4f6;
  white-space: pre-wrap;
  overflow-x: auto;
  margin-bottom: 20px;
}

.erm-error-stack-title {
  font-size: 13px;
  font-weight: 600;
  color: #a1a1aa;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  margin-bottom: 8px;
}

.erm-error-stack {
  background: #09090b;
  padding: 16px;
  border-radius: 8px;
  font-family: "SFMono-Regular", Consolas, "Liberation Mono", Menlo, monospace;
  font-size: 12px;
  line-height: 1.6;
  color: #a1a1aa;
  white-space: pre-wrap;
  overflow-x: auto;
  max-height: 250px;
  border: 1px solid rgba(255, 255, 255, 0.04);
}

.erm-error-footer {
  padding: 16px 24px;
  border-top: 1px solid rgba(255, 255, 255, 0.08);
  font-size: 12px;
  color: #71717a;
  background: #1b1b1f;
  display: flex;
  align-items: center;
  gap: 8px;
}

.erm-error-footer svg {
  width: 14px;
  height: 14px;
  fill: currentColor;
}
.erm-snippet-line {
  display: flex;
  font-family: "SFMono-Regular", Consolas, "Liberation Mono", Menlo, monospace;
  font-size: 13px;
  line-height: 1.6;
  border-radius: 4px;
}
.erm-snippet-line-error {
  background: rgba(239, 68, 68, 0.1);
}
.erm-snippet-ln {
  width: 40px;
  color: #71717a;
  text-align: right;
  padding-right: 12px;
  user-select: none;
}
.erm-snippet-marker {
  width: 15px;
  color: #ef4444;
  font-weight: bold;
  user-select: none;
}
.erm-snippet-code {
  color: #f3f4f6;
  white-space: pre;
}
.erm-snippet-line-error .erm-snippet-code {
  color: #fca5a5;
  font-weight: 500;
}
.erm-snippet-container {
  margin-bottom: 20px;
}
  `;

  window.__erm_show_error_overlay = function(err) {
    let overlay = document.getElementById('erm-error-overlay');
    if (overlay) overlay.remove();

    let style = document.getElementById('erm-error-overlay-styles');
    if (!style) {
      style = document.createElement('style');
      style.id = 'erm-error-overlay-styles';
      style.textContent = OVERLAY_CSS;
      (document.head || document.documentElement).appendChild(style);
    }

    overlay = document.createElement('div');
    overlay.id = 'erm-error-overlay';

    const card = document.createElement('div');
    card.className = 'erm-error-card';

    const header = document.createElement('div');
    header.className = 'erm-error-header';
    header.innerHTML = `
      <span class="erm-error-badge">${err.type || 'Error'}</span>
      <span class="erm-error-file">${err.file || ''}</span>
      <button class="erm-error-close-btn" onclick="document.getElementById('erm-error-overlay').remove()">
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <line x1="18" y1="6" x2="6" y2="18"></line>
          <line x1="6" y1="6" x2="18" y2="18"></line>
        </svg>
      </button>
    `;

    const body = document.createElement('div');
    body.className = 'erm-error-body';

    const title = document.createElement('h2');
    title.className = 'erm-error-title';
    title.textContent = err.title || 'An error occurred';

    const msg = document.createElement('div');
    msg.className = 'erm-error-msg';
    msg.textContent = err.message || '';

    body.appendChild(title);
    body.appendChild(msg);

    if (err.stack) {
      const stackTitle = document.createElement('div');
      stackTitle.className = 'erm-error-stack-title';
      stackTitle.textContent = 'Call Stack';
      
      const stack = document.createElement('pre');
      stack.className = 'erm-error-stack';
      stack.textContent = err.stack;

      body.appendChild(stackTitle);
      body.appendChild(stack);
    }

    const footer = document.createElement('div');
    footer.className = 'erm-error-footer';
    footer.innerHTML = `
      <svg viewBox="0 0 20 20"><path d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-7-4a1 1 0 11-2 0 1 1 0 012 0zM9 9a1 1 0 000 2v3a1 1 0 001 1h1a1 1 0 100-2v-3a1 1 0 00-1-1H9z" fill-rule="evenodd" clip-rule="evenodd"></path></svg>
      <span>Fix the issue in your code to automatically reload/clear.</span>
    `;

    card.appendChild(header);
    card.appendChild(body);
    card.appendChild(footer);
    overlay.appendChild(card);
    
    const escHandler = (e) => {
      if (e.key === 'Escape') {
        overlay.remove();
        document.removeEventListener('keydown', escHandler);
      }
    };
    document.addEventListener('keydown', escHandler);

    (document.body || document.documentElement).appendChild(overlay);

    const escapeHtml = (str) => str.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

    if (window.__erm_filename && err.file) {
      let resolvePromise;
      let compiledCol = 0;
      
      if (err.origLine !== undefined && err.origLine !== null) {
        const colMatch = err.file.match(/:(\d+)(?::(\d+))?$/) || (err.stack && err.stack.match(/(?::|\()(\d+)(?::(\d+))?\)?$/m));
        if (colMatch) {
          compiledCol = colMatch[2] ? parseInt(colMatch[2], 10) : 0;
        }
        resolvePromise = Promise.resolve(err.origLine);
      } else {
        const match = err.file.match(/(?::|\()(\d+)(?::(\d+))?\)?$/) || (err.stack && err.stack.match(/(?::|\()(\d+)(?::(\d+))?\)?$/m));
        if (match) {
          const compiledLine = parseInt(match[1], 10);
          compiledCol = match[2] ? parseInt(match[2], 10) : 0;
          
          resolvePromise = fetch(location.href)
            .then(r => r.text())
            .then(html => {
              const lines = html.split('\n');
              let origLine = null;
              for (let idx = compiledLine - 1; idx >= 0; idx--) {
                const lineContent = lines[idx] || '';
                const commentMatch = lineContent.match(/\/\/\s*line:(\d+)\s*$/);
                if (commentMatch) {
                  origLine = parseInt(commentMatch[1], 10);
                  break;
                }
              }
              return origLine;
            });
        }
      }
      
      if (resolvePromise) {
        resolvePromise
          .then(origLine => {
            if (origLine !== null && origLine !== undefined) {
              return fetch('/__erm_src/' + window.__erm_filename)
                .then(r => r.text())
                .then(src => {
                  const srcLines = src.split('\n');
                  const start = Math.max(0, origLine - 3);
                  const end = Math.min(srcLines.length, origLine + 2);
                  
                  let snippetHtml = '';
                  for (let i = start; i < end; i++) {
                    const ln = i + 1;
                    const isErrLine = ln === origLine;
                    const lineText = srcLines[i] || '';
                    snippetHtml += `<div class="erm-snippet-line${isErrLine ? ' erm-snippet-line-error' : ''}">` +
                      `<span class="erm-snippet-ln">${ln}</span>` +
                      `<span class="erm-snippet-marker">${isErrLine ? '>' : ' '}</span>` +
                      `<span class="erm-snippet-code">${escapeHtml(lineText)}</span>` +
                      `</div>`;
                  }
                  
                  const fileEl = card.querySelector('.erm-error-file');
                  if (fileEl) {
                    fileEl.textContent = `${window.__erm_filename}:${origLine}${compiledCol ? ':' + compiledCol : ''}`;
                  }
                  
                  const bodyEl = card.querySelector('.erm-error-body');
                  let snippetContainer = bodyEl.querySelector('.erm-snippet-container');
                  if (!snippetContainer) {
                    snippetContainer = document.createElement('div');
                    snippetContainer.className = 'erm-snippet-container';
                    
                    const snippetTitle = document.createElement('div');
                    snippetTitle.className = 'erm-error-stack-title';
                    snippetTitle.textContent = 'Source Code';
                    snippetContainer.appendChild(snippetTitle);
                    
                    const snippetPre = document.createElement('pre');
                    snippetPre.className = 'erm-error-stack';
                    snippetPre.style.background = '#09090b';
                    snippetPre.style.borderLeft = '4px solid #ef4444';
                    snippetPre.style.padding = '12px';
                    snippetPre.style.maxHeight = 'none';
                    snippetContainer.appendChild(snippetPre);
                    
                    const stackTitle = bodyEl.querySelector('.erm-error-stack-title');
                    if (stackTitle) {
                      bodyEl.insertBefore(snippetContainer, stackTitle);
                    } else {
                      bodyEl.appendChild(snippetContainer);
                    }
                  }
                  
                  snippetContainer.querySelector('pre').innerHTML = snippetHtml;
                });
            }
          })
          .catch(e => console.error("Error resolving source mapping:", e));
      }
    }
  };

  window.__hmr_hooks = window.__hmr_hooks || { dispose: [], accept: [] };
  window.hmr = {
    data: window.__hmr_data || {},
    accept: (cb) => window.__hmr_hooks.accept.push(cb),
    dispose: (cb) => window.__hmr_hooks.dispose.push(cb),
    invalidate: () => location.reload()
  };
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

  const originalElementAddEventListener = Element.prototype.addEventListener;
  Element.prototype.addEventListener = function(type, listener, options) {
    window.__hmr_listeners.push({ target: this, type, listener, options });
    return originalElementAddEventListener.call(this, type, listener, options);
  };

  const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
  const ws = new WebSocket(protocol + '//' + location.host + '/__hmr');
  ws.onmessage = (e) => {
    const data = JSON.parse(e.data);
    if (data.type === 'reload') {
      location.reload();
    } else if (data.type === 'update') {
      const path = data.path || 'unknown';
      console.log("[HMR] Update received for: " + path);

      if (path.endsWith('.css')) {
          let links = document.querySelectorAll('link[rel="stylesheet"]');
          let found = false;
          links.forEach(link => {
              if (link.href.includes(path)) {
                  link.href = path + '?t=' + new Date().getTime();
                  found = true;
              }
          });
          if (found) return;
      }

      fetch(location.href)
        .then(r => {
          if (!r.ok || r.status === 500) {
            return r.text().then(text => {
              const parser = new DOMParser();
              const doc = parser.parseFromString(text, 'text/html');
              const bodyEl = doc.body || doc.documentElement;
              const file = bodyEl.getAttribute('data-compile-error-file');
              const message = bodyEl.getAttribute('data-compile-error-message');
              if (file && message) {
                if (typeof window.__erm_show_error_overlay === 'function') {
                  window.__erm_show_error_overlay({
                    type: 'Failed to compile',
                    file: file,
                    title: 'An error occurred during template compilation.',
                    message: message
                  });
                  const closeBtn = document.querySelector('.erm-error-close-btn');
                  if (closeBtn) closeBtn.style.display = 'none';
                }
              } else {
                console.error("Server compilation failed:", text);
              }
              throw new Error("Compilation error overlay shown");
            });
          }
          return r.text();
        })
        .then(html => {
          const hasOverlay = !!document.getElementById('erm-error-overlay');
          const overlay = document.getElementById('erm-error-overlay');
          if (overlay) overlay.remove();
          const styleOverlay = document.getElementById('erm-error-overlay-styles');
          if (styleOverlay) styleOverlay.remove();

          if (hasOverlay || !document.querySelector('script.__erm_script') || (document.body.firstElementChild && document.body.firstElementChild.id === 'erm-error-overlay')) {
            location.reload();
            return;
          }

          const parser = new DOMParser();
          const doc = parser.parseFromString(html, 'text/html');
          document.title = doc.title;

          function morph(oldNode, newNode) {
            if (oldNode.nodeType !== newNode.nodeType || oldNode.tagName !== newNode.tagName) {
              oldNode.replaceWith(newNode.cloneNode(true));
              return;
            }
            if (oldNode.nodeType === Node.TEXT_NODE) {
              if (oldNode.textContent !== newNode.textContent) oldNode.textContent = newNode.textContent;
              return;
            }
            const oldAttrs = oldNode.attributes;
            const newAttrs = newNode.attributes;
            if (oldAttrs && newAttrs) {
              for (let i = 0; i < newAttrs.length; i++) {
                const attr = newAttrs[i];
                if (oldNode.getAttribute(attr.name) !== attr.value) oldNode.setAttribute(attr.name, attr.value);
              }
              for (let i = 0; i < oldAttrs.length; i++) {
                const attr = oldAttrs[i];
                if (!newNode.hasAttribute(attr.name)) oldNode.removeAttribute(attr.name);
              }
            }
            const oldChildren = Array.from(oldNode.childNodes);
            const newChildren = Array.from(newNode.childNodes);
            const max = Math.max(oldChildren.length, newChildren.length);
            for (let i = 0; i < max; i++) {
              if (i >= oldChildren.length) {
                oldNode.appendChild(newChildren[i].cloneNode(true));
              } else if (i >= newChildren.length) {
                oldNode.removeChild(oldChildren[i]);
              } else {
                morph(oldChildren[i], newChildren[i]);
              }
            }
          }

          const newStyles = doc.querySelectorAll('style');
          if (newStyles.length > 0) {
              let styleContainer = document.getElementById('__erm_styles');
              if (!styleContainer) {
                  styleContainer = document.createElement('div');
                  styleContainer.id = '__erm_styles';
                  document.head.appendChild(styleContainer);
              }
              styleContainer.innerHTML = '';
              newStyles.forEach(s => styleContainer.appendChild(s.cloneNode(true)));
          }

          window.__hmr_hooks.dispose.forEach(cb => { try { cb(window.hmr.data); } catch(err) {} });
          window.__hmr_hooks.dispose = [];
          
          const oldAccepts = window.__hmr_hooks.accept;
          window.__hmr_hooks.accept = [];

          window.__hmr_intervals.forEach(clearInterval);
          window.__hmr_intervals = [];
          window.__hmr_listeners.forEach(({ target, type, listener, options }) => {
            target.removeEventListener(type, listener, options);
            if (target.__erm_listener_added) delete target.__erm_listener_added;
          });
          window.__hmr_listeners = [];

          // Clean up old erm scripts
          document.querySelectorAll('script.__erm_script').forEach(s => s.remove());

          morph(document.body, doc.body);

          const newScripts = doc.querySelectorAll('script');
          newScripts.forEach(s => {
            if (s.textContent.includes("__hmr_initialized")) return;
            const newScript = document.createElement('script');
            newScript.text = s.innerHTML;
            if (s.className) newScript.className = s.className;
            try {
              if (s.src) {
                let sUrl = new URL(s.src, location.href);
                sUrl.searchParams.set('t', new Date().getTime());
                newScript.src = sUrl.href;
                document.head.appendChild(newScript);
              } else {
                (0, eval)(s.innerHTML);
              }
            } catch (err) {
              console.error("[HMR] Script evaluation failed:", err);
              if (typeof window.__erm_show_error_overlay === 'function') {
                let file = window.__erm_filename || location.pathname;
                let stack = err.stack || '';
                let origLine = null;
                const match = stack.match(/(?::|\()(\d+)(?::(\d+))?\)?$/m) || stack.match(/(?::|\()(\d+)(?::(\d+))?\)?$/);
                if (match) {
                  const inlineLine = parseInt(match[1], 10);
                  const inlineCol = match[2] ? parseInt(match[2], 10) : 0;
                  const scriptLines = s.innerHTML.split('\n');
                  for (let idx = inlineLine - 1; idx >= 0; idx--) {
                    const lineContent = scriptLines[idx] || '';
                    const commentMatch = lineContent.match(/\/\/\s*line:(\d+)\s*$/);
                    if (commentMatch) {
                      origLine = parseInt(commentMatch[1], 10);
                      break;
                    }
                  }
                  if (origLine !== null) {
                    file = `${file}:${origLine}${inlineCol ? ':' + inlineCol : ''}`;
                  }
                }
                window.__erm_show_error_overlay({
                  type: 'Runtime Error',
                  file: file,
                  title: err.name || 'SyntaxError',
                  message: err.message || 'Invalid or unexpected token',
                  stack: stack,
                  origLine: origLine
                });
              }
            }
          });
          document.dispatchEvent(new Event('DOMContentLoaded'));
          window.dispatchEvent(new Event('load'));
          
          oldAccepts.forEach(cb => { try { cb(); } catch(err) {} });
          
          if (window.__erm_update) window.__erm_update();
        });
    }
  };

  const checkInitialCompileError = () => {
    const bodyEl = document.body || document.documentElement;
    if (bodyEl) {
      const file = bodyEl.getAttribute('data-compile-error-file');
      const message = bodyEl.getAttribute('data-compile-error-message');
      if (file && message) {
        window.__erm_show_error_overlay({
          type: 'Failed to compile',
          file: file,
          title: 'An error occurred during template compilation.',
          message: message
        });
        const closeBtn = document.querySelector('.erm-error-close-btn');
        if (closeBtn) closeBtn.style.display = 'none';
      }
    }
  };

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', checkInitialCompileError);
  } else {
    checkInitialCompileError();
  }
})();
