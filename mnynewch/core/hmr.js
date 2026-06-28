(function() {
  if (window.__hmr_initialized) return;
  window.__hmr_initialized = true;
  console.log("[HMR] Initialized");

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
        .then(r => r.text())
        .then(html => {
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
            if (s.src) {
               let sUrl = new URL(s.src, location.href);
               sUrl.searchParams.set('t', new Date().getTime());
               newScript.src = sUrl.href;
            }
            document.head.appendChild(newScript);
          });
          document.dispatchEvent(new Event('DOMContentLoaded'));
          window.dispatchEvent(new Event('load'));
          
          oldAccepts.forEach(cb => { try { cb(); } catch(err) {} });
          
          if (window.__erm_update) window.__erm_update();
        });
    }
  };
})();
