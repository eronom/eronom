/**
 * Eronom Hot Module Replacement (HMR) Client
 * High-performance Hot Module Replacement (HMR) architecture.
 * Features:
 * - Pure ES class architecture (HMRClient, HMRContext)
 * - Shadow DOM isolated <erm-error-overlay>
 * - In-place CSS hot updates without DOM re-render or state loss
 * - Standard hot.accept(), hot.dispose(), hot.data, hot.invalidate() lifecycle
 * - Clean WebSocket reconnect with heartbeat ping/pong
 */

// --- 1. Shadow DOM Error Overlay ---

class ErmErrorOverlay extends HTMLElement {
  constructor(err) {
    super();
    this.attachShadow({ mode: 'open' });
    this.err = err || {};
    this.render();
  }

  static get observedAttributes() {
    return [];
  }

  connectedCallback() {
    this.onKeyDown = (e) => {
      if (e.key === 'Escape') {
        this.close();
      }
    };
    addEventListener('keydown', this.onKeyDown);
  }

  disconnectedCallback() {
    removeEventListener('keydown', this.onKeyDown);
  }

  close() {
    this.remove();
  }

  escapeHtml(str) {
    if (!str) return '';
    return String(str)
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;')
      .replace(/'/g, '&#39;');
  }

  formatCodeFrame(frame) {
    if (!frame) return '';
    return frame
      .split('\n')
      .map((line) => {
        const isHighlight = line.trimStart().startsWith('>') || line.includes('^');
        const escaped = this.escapeHtml(line);
        return isHighlight
          ? `<span class="frame-line highlight">${escaped}</span>`
          : `<span class="frame-line">${escaped}</span>`;
      })
      .join('\n');
  }

  render() {
    const err = this.err;
    const title = err.title || err.type || 'Build Error';
    const message = this.escapeHtml(err.message || 'An unknown error occurred');
    const file = this.escapeHtml(err.id || err.file || '');
    const frame = this.formatCodeFrame(err.frame || '');
    const stack = this.escapeHtml(err.stack || '');

    this.shadowRoot.innerHTML = `
      <style>
        :host {
          position: fixed;
          top: 0;
          left: 0;
          width: 100vw;
          height: 100vh;
          box-sizing: border-box;
          z-index: 999999;
          --bg-backdrop: rgba(10, 10, 14, 0.75);
          --card-bg: #141418;
          --card-border: rgba(255, 255, 255, 0.08);
          --text-main: #f3f3f6;
          --text-muted: #8e8e9f;
          --red-badge: #e03131;
          --red-bg: rgba(224, 49, 49, 0.12);
          --red-border: rgba(224, 49, 49, 0.3);
          --code-bg: #0d0d11;
          --font-sans: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
          --font-mono: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
          background: var(--bg-backdrop);
          backdrop-filter: blur(12px);
          -webkit-backdrop-filter: blur(12px);
          display: flex;
          align-items: center;
          justify-content: center;
          padding: 24px;
          font-family: var(--font-sans);
          color: var(--text-main);
          line-height: 1.5;
        }

        * {
          box-sizing: border-box;
        }

        .overlay-card {
          background: var(--card-bg);
          border: 1px solid var(--card-border);
          border-radius: 14px;
          box-shadow: 0 20px 50px rgba(0, 0, 0, 0.6), 0 0 0 1px rgba(255, 255, 255, 0.05);
          width: 100%;
          max-width: 900px;
          max-height: 88vh;
          overflow-y: auto;
          padding: 28px;
          position: relative;
        }

        .header {
          display: flex;
          align-items: center;
          justify-content: space-between;
          margin-bottom: 16px;
        }

        .header-left {
          display: flex;
          align-items: center;
          gap: 12px;
        }

        .badge {
          background: var(--red-bg);
          color: var(--red-badge);
          border: 1px solid var(--red-border);
          font-size: 11px;
          font-weight: 700;
          text-transform: uppercase;
          letter-spacing: 0.06em;
          padding: 4px 10px;
          border-radius: 6px;
        }

        .file-path {
          font-family: var(--font-mono);
          font-size: 13px;
          color: var(--text-muted);
          word-break: break-all;
        }

        .close-btn {
          background: transparent;
          border: 1px solid var(--card-border);
          color: var(--text-muted);
          width: 30px;
          height: 30px;
          border-radius: 8px;
          display: flex;
          align-items: center;
          justify-content: center;
          cursor: pointer;
          font-size: 16px;
          transition: all 0.15s ease;
        }

        .close-btn:hover {
          color: var(--text-main);
          border-color: rgba(255, 255, 255, 0.2);
          background: rgba(255, 255, 255, 0.05);
        }

        .message {
          font-size: 17px;
          font-weight: 600;
          color: #ff6b6b;
          margin: 0 0 18px 0;
          word-break: break-word;
        }

        .code-block {
          background: var(--code-bg);
          border: 1px solid rgba(255, 255, 255, 0.06);
          border-radius: 10px;
          padding: 16px;
          overflow-x: auto;
          font-family: var(--font-mono);
          font-size: 13px;
          line-height: 1.6;
          margin-bottom: 18px;
        }

        .frame-line {
          display: block;
          white-space: pre;
          color: #d1d1db;
        }

        .frame-line.highlight {
          color: #ff8787;
          background: rgba(255, 107, 107, 0.12);
          border-left: 3px solid #ff6b6b;
          margin-left: -16px;
          padding-left: 13px;
        }

        .stack-block {
          margin-top: 14px;
        }

        .stack-summary {
          font-size: 12px;
          color: var(--text-muted);
          cursor: pointer;
          user-select: none;
          margin-bottom: 8px;
        }

        .stack-content {
          background: var(--code-bg);
          border: 1px solid rgba(255, 255, 255, 0.06);
          border-radius: 8px;
          padding: 14px;
          font-family: var(--font-mono);
          font-size: 12px;
          color: #9c9cae;
          white-space: pre-wrap;
          overflow-x: auto;
        }

        .footer {
          margin-top: 20px;
          font-size: 12px;
          color: var(--text-muted);
          display: flex;
          justify-content: space-between;
          align-items: center;
        }

        .kbd-hint {
          display: inline-flex;
          align-items: center;
          gap: 4px;
        }

        kbd {
          background: rgba(255, 255, 255, 0.08);
          border: 1px solid rgba(255, 255, 255, 0.15);
          border-radius: 4px;
          padding: 2px 6px;
          font-family: var(--font-mono);
          font-size: 11px;
        }
      </style>

      <div class="overlay-card">
        <div class="header">
          <div class="header-left">
            <span class="badge">[eronom] ${title}</span>
            ${file ? `<span class="file-path">${file}</span>` : ''}
          </div>
          <button class="close-btn" id="close" title="Dismiss (Esc)">&times;</button>
        </div>

        <div class="message">${message}</div>

        ${frame ? `<div class="code-block">${frame}</div>` : ''}

        ${stack ? `
          <details class="stack-block">
            <summary class="stack-summary">View Stack Trace</summary>
            <div class="stack-content">${stack}</div>
          </details>
        ` : ''}

        <div class="footer">
          <span>Fix the problem in your editor to clear this screen automatically.</span>
          <span class="kbd-hint">Press <kbd>Esc</kbd> to dismiss</span>
        </div>
      </div>
    `;

    this.shadowRoot.getElementById('close')?.addEventListener('click', () => this.close());
  }
}

if (!customElements.get('erm-error-overlay')) {
  customElements.define('erm-error-overlay', ErmErrorOverlay);
}

// --- 2. HMRContext (Module Hot Context) ---

export class HMRContext {
  constructor(client, ownerPath) {
    this._client = client;
    this._ownerPath = ownerPath;

    // Reset callbacks on new context instantiation
    const mod = this._client.hotModulesMap.get(ownerPath);
    if (mod) {
      mod.callbacks = [];
    }

    if (!this._client.dataMap.has(ownerPath)) {
      this._client.dataMap.set(ownerPath, {});
    }
  }

  /**
   * Persistent state dictionary preserved across hot updates.
   */
  get data() {
    return this._client.dataMap.get(this._ownerPath);
  }

  /**
   * Accept updates for this module or explicit dependencies.
   * @param {string|string[]|Function} [deps]
   * @param {Function} [callback]
   */
  accept(deps, callback) {
    if (typeof deps === 'function' || !deps) {
      // Self-accepting: hot.accept(() => {})
      this._acceptDeps([this._ownerPath], ([newMod]) => deps && deps(newMod));
    } else if (typeof deps === 'string') {
      // Single dependency
      this._acceptDeps([deps], ([newMod]) => callback && callback(newMod));
    } else if (Array.isArray(deps)) {
      // Multiple dependencies
      this._acceptDeps(deps, callback);
    }
  }

  acceptExports(_exports, callback) {
    this._acceptDeps([this._ownerPath], ([newMod]) => callback && callback(newMod));
  }

  /**
   * Clean up side effects before module re-evaluation.
   * @param {(data: Record<string, any>) => void} cb
   */
  dispose(cb) {
    this._client.disposeMap.set(this._ownerPath, cb);
  }

  /**
   * Called when a module is pruned from the graph.
   * @param {(data: Record<string, any>) => void} cb
   */
  prune(cb) {
    this._client.pruneMap.set(this._ownerPath, cb);
  }

  /**
   * Mark this module as cannot-be-hot-updated and request full reload.
   */
  invalidate(reason) {
    console.log(`[eronom] [invalidate] ${this._ownerPath}${reason ? `: ${reason}` : ''}`);
    this._client.send({
      type: 'custom',
      event: 'eronom:invalidate',
      data: { path: this._ownerPath, reason }
    });
    location.reload();
  }

  on(event, cb) {
    const list = this._client.customListenersMap.get(event) || [];
    list.push(cb);
    this._client.customListenersMap.set(event, list);
  }

  off(event, cb) {
    const list = this._client.customListenersMap.get(event);
    if (list) {
      this._client.customListenersMap.set(event, list.filter(fn => fn !== cb));
    }
  }

  send(event, data) {
    this._client.send({ type: 'custom', event, data });
  }

  _acceptDeps(deps, callback) {
    let mod = this._client.hotModulesMap.get(this._ownerPath);
    if (!mod) {
      mod = { id: this._ownerPath, callbacks: [] };
      this._client.hotModulesMap.set(this._ownerPath, mod);
    }
    mod.callbacks.push({
      deps,
      fn: callback || (() => {})
    });
  }
}

// --- 3. HMRClient Core ---

export class HMRClient {
  constructor() {
    this.hotModulesMap = new Map();
    this.disposeMap = new Map();
    this.pruneMap = new Map();
    this.dataMap = new Map();
    this.customListenersMap = new Map();
    this.ws = null;
    this.reconnectAttempts = 0;
    this.lastSeq = 0;
    this.activeOverlay = null;

    this.connect();
    this.checkInitialCompilerError();
  }

  connect() {
    const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsUrl = `${protocol}//${location.host}/__hmr`;

    try {
      this.ws = new WebSocket(wsUrl);
    } catch (err) {
      this.scheduleReconnect();
      return;
    }

    this.ws.onopen = () => {
      this.reconnectAttempts = 0;
    };

    this.ws.onmessage = (e) => {
      this.handleMessage(e.data);
    };

    this.ws.onclose = () => {
      this.scheduleReconnect();
    };

    this.ws.onerror = () => {
      // Handled in onclose
    };
  }

  scheduleReconnect() {
    if (this.reconnectAttempts > 30) return;
    const delay = Math.min(1000 * Math.pow(1.5, this.reconnectAttempts), 5000);
    this.reconnectAttempts++;
    setTimeout(() => this.connect(), delay);
  }

  send(payload) {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(typeof payload === 'string' ? payload : JSON.stringify(payload));
    }
  }

  createContext(ownerPath) {
    return new HMRContext(this, ownerPath);
  }

  clearError() {
    if (this.activeOverlay) {
      this.activeOverlay.remove();
      this.activeOverlay = null;
    }
    document.querySelectorAll('erm-error-overlay').forEach(el => el.remove());
  }

  showError(err) {
    this.clearError();
    const overlay = new ErmErrorOverlay(err);
    document.body.appendChild(overlay);
    this.activeOverlay = overlay;
  }

  checkInitialCompilerError() {
    if (typeof document === 'undefined') return;
    const body = document.body;
    if (!body) return;

    const errorFile = body.getAttribute('data-compile-error-file');
    const errorMessage = body.getAttribute('data-compile-error-message');
    if (errorFile && errorMessage) {
      this.showError({
        type: 'Build Error',
        file: errorFile,
        message: errorMessage,
        id: errorFile
      });
    }
  }

  handleMessage(raw) {
    let payload;
    try {
      payload = JSON.parse(raw);
    } catch (err) {
      return;
    }

    switch (payload.type) {
      case 'connected':
        console.log('[eronom] connected.');
        break;

      case 'ping':
        this.send({ type: 'pong' });
        break;

      case 'pong':
        break;

      case 'error':
        this.showError(payload.err || payload);
        break;

      case 'full-reload':
        console.log('[eronom] [full-reload] ' + (payload.path || payload.reason || ''));
        location.reload();
        break;

      case 'update':
        this.clearError();
        const updates = payload.updates || (payload.path ? [{
          type: payload.path.endsWith('.css') ? 'css-update' : 'js-update',
          path: payload.path,
          acceptedPath: payload.path,
          timestamp: Date.now()
        }] : []);

        for (const update of updates) {
          if (update.type === 'css-update' || (update.path && update.path.endsWith('.css'))) {
            this.updateCss(update.path, update.timestamp || Date.now());
          } else {
            this.updateJs(update.path, update.acceptedPath || update.path, update.timestamp || Date.now());
          }
        }
        break;

      case 'prune':
        if (payload.paths && Array.isArray(payload.paths)) {
          for (const path of payload.paths) {
            const pruner = this.pruneMap.get(path);
            if (pruner) {
              try { pruner(this.dataMap.get(path)); } catch (e) { console.error(e); }
            }
            this.hotModulesMap.delete(path);
            this.disposeMap.delete(path);
            this.pruneMap.delete(path);
            this.dataMap.delete(path);
          }
        }
        break;

      case 'custom':
        if (payload.event) {
          const listeners = this.customListenersMap.get(payload.event) || [];
          for (const listener of listeners) {
            try { listener(payload.data); } catch (e) { console.error(e); }
          }
        }
        break;

      default:
        break;
    }
  }

  /**
   * In-place CSS hot reloading without DOM mutation or state loss.
   */
  updateCss(rawPath, timestamp) {
    const cleanPath = rawPath.split('?')[0];
    const isGlobalErmCss = cleanPath.includes('eronom.toml') || cleanPath.includes('global') || cleanPath.endsWith('.css');

    let updated = false;

    // 1. Update <link rel="stylesheet">
    const links = document.querySelectorAll('link[rel="stylesheet"]');
    links.forEach((link) => {
      const href = link.getAttribute('href');
      if (href && (href.includes(cleanPath) || (isGlobalErmCss && href.includes('ermcss')))) {
        const url = new URL(link.href, location.href);
        url.searchParams.set('t', String(timestamp));
        link.href = url.href;
        updated = true;
      }
    });

    // 2. Update <style id="__erm_styles"> and <style id="__erm_scoped_styles">
    fetch(location.href, { headers: { 'Accept': 'text/html' } })
      .then(r => r.text())
      .then(html => {
        const doc = new DOMParser().parseFromString(html, 'text/html');
        const newStyles = doc.getElementById('__erm_styles');
        const curStyles = document.getElementById('__erm_styles');
        if (newStyles && curStyles && newStyles.tagName !== 'LINK') {
          curStyles.textContent = newStyles.textContent;
          updated = true;
        }
        const newScoped = doc.getElementById('__erm_scoped_styles');
        const curScoped = document.getElementById('__erm_scoped_styles');
        if (newScoped && curScoped) {
          curScoped.textContent = newScoped.textContent;
          updated = true;
        }
      })
      .catch(() => {});

    console.log(`[eronom] [css-update] ${cleanPath}`);
  }

  /**
   * JavaScript & ERM Component hot reloading.
   */
  async updateJs(path, acceptedPath, timestamp) {
    const targetPath = acceptedPath || path;
    const mod = this.hotModulesMap.get(targetPath);

    // Run registered disposers
    const disposer = this.disposeMap.get(targetPath);
    if (disposer) {
      try {
        disposer(this.dataMap.get(targetPath));
      } catch (err) {
        console.error(`[eronom] Error in hot.dispose for ${targetPath}:`, err);
      }
    }

    if (mod && mod.callbacks.length > 0) {
      try {
        const url = new URL(targetPath, location.origin);
        url.searchParams.set('t', String(timestamp));
        const newMod = await import(url.href);

        for (const cb of mod.callbacks) {
          cb.fn([newMod]);
        }
        console.log(`[eronom] [hmr] ${targetPath}`);
        return;
      } catch (err) {
        console.error(`[eronom] Failed to hot update ${targetPath}:`, err);
        this.showError({
          type: 'Runtime Error',
          file: targetPath,
          message: err.message,
          stack: err.stack
        });
        return;
      }
    }

    // ERM Hot Module Replacement boundary:
    // If runtime HMR handler is available, hot-update the ERM component/page in-place!
    if (typeof window !== 'undefined' && typeof window.__erm_apply_hmr === 'function') {
      try {
        await window.__erm_apply_hmr(targetPath, timestamp);
        return;
      } catch (err) {
        console.error(`[eronom] [hmr] Error during ERM hot update:`, err);
      }
    }

    // Fallback only if no ERM HMR handler is registered
    console.log(`[eronom] [full-reload] ${targetPath}`);
    location.reload();
  }
}

// --- 4. Initialization & Export ---

export const hmrClient = new HMRClient();

if (typeof window !== 'undefined') {
  window.hmrClient = hmrClient;
  window.hmr = hmrClient;
}

export function createHotContext(ownerPath) {
  return hmrClient.createContext(ownerPath);
}

export function showError(err) {
  hmrClient.showError(err);
}

export function clearError() {
  hmrClient.clearError();
}

// Global runtime error listeners for development overlay
addEventListener('error', (event) => {
  const error = event.error || { message: event.message };
  const stack = error.stack || '';
  const filename = event.filename ? event.filename.replace(location.origin, '') : 'unknown';
  hmrClient.showError({
    type: 'Runtime Error',
    file: filename + (event.lineno ? `:${event.lineno}:${event.colno}` : ''),
    title: error.name || 'Error',
    message: error.message || event.message,
    stack: stack
  });
});

addEventListener('unhandledrejection', (event) => {
  const reason = event.reason || {};
  const stack = reason.stack || '';
  hmrClient.showError({
    type: 'Unhandled Rejection',
    file: 'Promise Rejection',
    title: reason.name || 'Error',
    message: reason.message || String(reason),
    stack: stack
  });
});
