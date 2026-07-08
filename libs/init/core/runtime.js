  window.__hmr_data = window.__hmr_data || { states: {} };
  if (!window.__hmr_data.states) window.__hmr_data.states = {};

  window.__erm_b64utf8 = function (str) {
    if (!str) return '';
    return decodeURIComponent(escape(window.atob(str)));
  };

  window.__erm_escape = function (val) {
    if (val === null || val === undefined) return '';
    return String(val)
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;')
      .replace(/'/g, '&#39;');
  };


  // Fine-grained Reactivity Core
  let activeListener = null;
  const listenerStack = [];

  function pushListener(listener) {
    listenerStack.push(activeListener);
    activeListener = listener;
  }

  function popListener() {
    activeListener = listenerStack.pop();
  }

  const queuedBindings = new Set();
  let updateScheduled = false;

  function queueUpdate(binding) {
    queuedBindings.add(binding);
    if (!updateScheduled) {
      updateScheduled = true;
      queueMicrotask(flushUpdates);
    }
  }

  function flushUpdates() {
    // Sort queued bindings to process structural elements (providers, conditional, loop blocks) before text/attribute bindings
    const getPriority = (b) => {
      if (!b.id) return 99;
      if (b.isProvider || b.id.startsWith("erm-prov-")) return 0;
      if (b.id.startsWith("erm-if-")) return 1;
      if (b.id.startsWith("erm-for-")) return 2;
      return 10;
    };

    const bindingsToRun = Array.from(queuedBindings).sort((a, b) => {
      const priA = getPriority(a);
      const priB = getPriority(b);
      if (priA !== priB) return priA - priB;
      return window.__erm_bindings.indexOf(a) - window.__erm_bindings.indexOf(b);
    });

    queuedBindings.clear();
    updateScheduled = false;

    bindingsToRun.forEach(b => {
      // Clear old dependencies before re-evaluation (supports dynamic conditional branching)
      if (b.deps) {
        b.deps.forEach(subscribersSet => {
          subscribersSet.delete(b);
        });
        b.deps.clear();
      }

      pushListener(b);
      try {
        window.__current_eval_id = b.id;
        if (typeof b.update === 'function') {
          b.update();
        } else {
          let val = b.get();
          let el = document.getElementById(b.id);
          let strVal = val === undefined ? '' : String(val);
          if (el && el.innerText !== strVal) {
            b.last = val;
            el.innerText = strVal;
          }
        }
        if (!b.alwaysRun) {
          b.initialized = true;
        }
      } catch (e) {
        console.error("Binding update failed:", e);
        if (typeof window.__erm_show_error_overlay === 'function') {
          window.__erm_show_error_overlay({
            type: 'Reactivity Error',
            file: `Binding element: #${b.id}`,
            title: e.name || 'TypeError',
            message: e.message || String(e),
            stack: e.stack || ''
          });
        }
      } finally {
        window.__current_eval_id = null;
        popListener();
      }
    });

    if (typeof _initReactivity === 'function') {
      _initReactivity();
    }

    // Update any text/attribute bindings whose elements are now in the DOM
    window.__erm_bindings.forEach(b => {
      if (typeof b.update !== 'function' && b.id) {
        let el = document.getElementById(b.id);
        if (el) {
          pushListener(b);
          try {
            let val = b.get();
            let strVal = val === undefined ? '' : String(val);
            if (el.innerText !== strVal) {
              el.innerText = strVal;
            }
            if (!b.alwaysRun) {
              b.initialized = true;
            }
          } catch (e) {
            console.error("Delayed binding update failed:", e);
            if (typeof window.__erm_show_error_overlay === 'function') {
              window.__erm_show_error_overlay({
                type: 'Reactivity Error',
                file: `Binding element: #${b.id}`,
                title: e.name || 'TypeError',
                message: e.message || String(e),
                stack: e.stack || ''
              });
            }
          } finally {
            popListener();
          }
        }
      }
    });
  }

  class Signal {
    constructor(val, name) {
      this._val = val;
      this.id = name;
      this.defaultValue = val;
      this.subscribers = new Set();
    }

    get value() {
      if (activeListener) {
        this.subscribers.add(activeListener);
        activeListener.deps = activeListener.deps || new Set();
        activeListener.deps.add(this.subscribers);
      }
      if (Array.isArray(this._val)) {
        return this.makeArrayProxy(this._val);
      }
      return this._val;
    }

    set value(newVal) {
      if (this._val !== newVal) {
        this._val = newVal;
        if (this.id) {
          window.__hmr_data.states[this.id] = newVal;
        }
        this.subscribers.forEach(b => queueUpdate(b));
      }
    }

    makeArrayProxy(arr) {
      const self = this;
      return new Proxy(arr, {
        get(target, prop) {
          let res = target[prop];
          if (typeof res === 'function') {
            const methods = ['push', 'pop', 'shift', 'unshift', 'splice', 'sort', 'reverse'];
            if (methods.includes(prop)) {
              return (...args) => {
                const result = target[prop].apply(target, args);
                if (self.id) {
                  window.__hmr_data.states[self.id] = self._val;
                }
                self.subscribers.forEach(b => queueUpdate(b));
                return result;
              };
            }
            return res.bind(target);
          }
          return res;
        },
        set(target, prop, newVal) {
          target[prop] = newVal;
          if (self.id) {
            window.__hmr_data.states[self.id] = self._val;
          }
          self.subscribers.forEach(b => queueUpdate(b));
          return true;
        }
      });
    }

    toString() { return this.value; }
    valueOf() { return this.value; }
    [Symbol.toPrimitive]() { return this.value; }
  }

  class DerivedSignal {
    constructor(getter, name) {
      this._getter = getter;
      this.id = name;
      this.defaultValue = undefined;
      this.subscribers = new Set();
    }

    get value() {
      if (activeListener) {
        this.subscribers.add(activeListener);
        activeListener.deps = activeListener.deps || new Set();
        activeListener.deps.add(this.subscribers);
      }
      return this._getter();
    }

    toString() { return this.value; }
    valueOf() { return this.value; }
    [Symbol.toPrimitive]() { return this.value; }
  }

  const statesRegistry = new Map();

  const useState = function (val, name) {
    if (name && statesRegistry.has(name)) {
      return statesRegistry.get(name);
    }
    if (name && window.__hmr_data.states[name] !== undefined) {
      val = window.__hmr_data.states[name];
    }

    let stateObj;
    if (typeof val === 'function') {
      stateObj = new DerivedSignal(val, name);
    } else {
      stateObj = new Signal(val, name);
    }

    if (name) {
      statesRegistry.set(name, stateObj);
    }
    return stateObj;
  };

  const effect = (fn) => {
    const binding = {
      id: 'effect-' + Math.random().toString(36).substr(2, 9),
      deps: new Set(),
      alwaysRun: true,
      update: fn
    };
    window.__erm_bindings.push(binding);
    pushListener(binding);
    try {
      fn();
    } finally {
      popListener();
    }
  };

  const useEffect = function (callback, depsFn) {
    let lastDeps = undefined;
    let hasRun = false;

    const binding = {
      id: 'effect-' + Math.random().toString(36).substr(2, 9),
      cleanup: null,
      alwaysRun: !depsFn,
      update: () => {
        if (typeof binding.cleanup === 'function') {
          try {
            binding.cleanup();
          } catch (e) {
            console.error("Effect cleanup failed:", e);
          }
          binding.cleanup = null;
        }

        let shouldRun = false;
        let currentDeps = undefined;

        if (depsFn) {
          try {
            currentDeps = depsFn();
          } catch (e) {
            console.error("Effect deps evaluation failed:", e);
          }
        }

        if (!hasRun) {
          shouldRun = true;
          hasRun = true;
        } else if (depsFn && currentDeps && lastDeps) {
          shouldRun = currentDeps.some((dep, idx) => dep !== lastDeps[idx]);
        } else if (!depsFn) {
          shouldRun = true;
        }

        lastDeps = currentDeps;

        if (shouldRun) {
          if (depsFn) {
            pushListener(null);
          }
          try {
            binding.cleanup = callback();
          } catch (e) {
            console.error("Effect callback failed:", e);
            if (typeof window.__erm_show_error_overlay === 'function') {
              window.__erm_show_error_overlay({
                type: 'Effect Error',
                file: 'useEffect',
                title: e.name || 'TypeError',
                message: e.message || String(e),
                stack: e.stack || ''
              });
            }
          } finally {
            if (depsFn) {
              popListener();
            }
          }
        }
        binding.initialized = true;
      }
    };

    window.__erm_bindings.push(binding);

    pushListener(binding);
    try {
      binding.update();
    } finally {
      popListener();
    }
  };

  const onMount = function (callback) {
    useEffect(callback, () => []);
  };

  const useParams = function () { return window.__erm_params || {}; };

  window.__erm_bindings = [];
  window.__erm_bindings.push = function (binding) {
    return Array.prototype.push.call(this, binding);
  };

  window.__erm_events = [];
  window.__erm_dynamic_events = {};
  let nextDynamicEventId = 0;

  window.__erm_register_event = function (event, handler) {
    const id = ++nextDynamicEventId;
    window.__erm_dynamic_events[id] = { event, handler };
    return `data-erm-evt-id="${id}"`;
  };

  window.__current_eval_id = null;

  window.__erm_update = function () {
    window.__erm_bindings.forEach(b => {
      if (!b.initialized || b.alwaysRun) {
        queuedBindings.add(b);
      }
    });
    if (!updateScheduled) {
      updateScheduled = true;
      queueMicrotask(flushUpdates);
    }
  };

  function _initReactivity() {
    window.__erm_events.forEach(ev => {
      let el = document.getElementById(ev.id);
      if (el && !el.__erm_listener_added) {
        el.addEventListener(ev.event, ev.handler);
        el.__erm_listener_added = true;
      }
    });
    document.querySelectorAll('[data-erm-evt-id]').forEach(el => {
      const id = el.getAttribute('data-erm-evt-id');
      if (el.__erm_evt_id !== id) {
        if (el.__erm_evt_listener && el.__erm_evt_type) {
          el.removeEventListener(el.__erm_evt_type, el.__erm_evt_listener);
        }
        const ev = window.__erm_dynamic_events[id];
        if (ev) {
          const wrapper = (event) => {
            ev.handler(event);
            if (typeof window.__erm_update === 'function') window.__erm_update();
          };
          el.addEventListener(ev.event, wrapper);
          el.__erm_evt_id = id;
          el.__erm_evt_type = ev.event;
          el.__erm_evt_listener = wrapper;
        }
      }
    });
  }

  // DOM Reconciliation/Diffing Helper
  function reconcileNodes(parent, newNodes) {
    const childNodes = Array.from(parent.childNodes);
    // Remove extra nodes
    for (let k = newNodes.length; k < childNodes.length; k++) {
      parent.removeChild(childNodes[k]);
    }
    // Update or append
    for (let k = 0; k < newNodes.length; k++) {
      let existing = parent.childNodes[k];
      let incoming = newNodes[k];
      if (!existing) {
        parent.appendChild(incoming);
      } else if (existing.nodeType !== incoming.nodeType) {
        parent.replaceChild(incoming, existing);
      } else if (existing.nodeType === Node.TEXT_NODE) {
        if (existing.nodeValue !== incoming.nodeValue) {
          existing.nodeValue = incoming.nodeValue;
        }
      } else if (existing.nodeType === Node.ELEMENT_NODE) {
        if (existing.tagName !== incoming.tagName || existing.id !== incoming.id) {
          parent.replaceChild(incoming, existing);
        } else {
          // Reconcile attributes
          for (let attr of Array.from(existing.attributes)) {
            if (!incoming.hasAttribute(attr.name)) {
              existing.removeAttribute(attr.name);
              if (attr.name === 'data-erm-evt-id') {
                if (existing.__erm_evt_listener && existing.__erm_evt_type) {
                  existing.removeEventListener(existing.__erm_evt_type, existing.__erm_evt_listener);
                  existing.__erm_evt_id = undefined;
                  existing.__erm_evt_type = undefined;
                  existing.__erm_evt_listener = undefined;
                }
              }
            }
          }
          for (let attr of Array.from(incoming.attributes)) {
            if (existing.getAttribute(attr.name) !== attr.value) {
              existing.setAttribute(attr.name, attr.value);
            }
          }
          // Sync input/textarea properties
          if (existing.tagName === 'INPUT' || existing.tagName === 'TEXTAREA') {
            let incomingVal = incoming.value;
            if (existing.tagName === 'TEXTAREA' && incoming.hasAttribute('value')) {
              incomingVal = incoming.getAttribute('value');
            }
            if (existing.value !== incomingVal) {
              existing.value = incomingVal;
            }
            if (existing.checked !== incoming.checked) {
              existing.checked = incoming.checked;
            }
          }
          // Recursively reconcile children
          reconcileNodes(existing, Array.from(incoming.childNodes));
        }
      }
    }
  }

  function initLoadingSwap() {
    const fallback = document.getElementById('erm-loading-fallback');
    const content = document.getElementById('erm-loading-content');
    const suspenses = document.querySelectorAll('.erm-suspense-container');
    
    if (!fallback && suspenses.length === 0) return;

    if (fallback && content) {
      fallback.style.display = 'block';
      content.style.display = 'none';
    }
    suspenses.forEach(s => {
      const fb = s.querySelector('.erm-suspense-fallback');
      const ct = s.querySelector('.erm-suspense-content');
      if (fb && ct) {
        fb.style.display = 'block';
        ct.style.display = 'none';
      }
    });

    const originalFetch = window.fetch;
    let activeInitFetches = 0;
    let finished = false;

    function checkLoadingFinished() {
      if (finished) return;
      setTimeout(() => {
        if (activeInitFetches <= 0) {
          finished = true;
          window.fetch = originalFetch;
          if (fallback && content) {
            fallback.style.display = 'none';
            content.style.display = 'contents';
          }
          suspenses.forEach(s => {
            const fb = s.querySelector('.erm-suspense-fallback');
            const ct = s.querySelector('.erm-suspense-content');
            if (fb && ct) {
              fb.style.display = 'none';
              ct.style.display = 'contents';
            }
          });
          if (typeof window.__erm_update === 'function') {
            window.__erm_update();
          }
        }
      }, 20);
    }

    window.fetch = function(...args) {
      if (finished) {
        return originalFetch(...args);
      }
      activeInitFetches++;
      return originalFetch(...args).finally(() => {
        activeInitFetches--;
        queueMicrotask(checkLoadingFinished);
      });
    };

    setTimeout(() => {
      checkLoadingFinished();
    }, 100);
  }

  initLoadingSwap();

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', () => {
      _initReactivity();
      window.__erm_update();
    });
  } else {
    _initReactivity();
    window.__erm_update();
  }
  setTimeout(() => {
    _initReactivity();
    window.__erm_update();
  }, 10);

  // Helper functions for template rendering, conditional branches, and loops
  window.__erm_render_template = function (template, evalFn) {
    return template.replace(/\{([^{}#/:][^{}]*)\}/g, (m, expr) => {
      try {
        let val = evalFn(expr);
        return val === undefined ? "" : val;
      } catch (e) { return ""; }
    });
  };

  window.__erm_register_for = function (anchorId, getCollection, templateB64, renderItem) {
    window.__erm_bindings.push({
      id: anchorId,
      update: () => {
        let anchor = document.getElementById(anchorId);
        if (anchor) {
          let items = [];
          try { items = getCollection(); } catch (e) { }
          if (!Array.isArray(items)) items = [];
          let itemsJson = JSON.stringify(items);
          if (anchor.__erm_last_items !== itemsJson) {
            anchor.__erm_last_items = itemsJson;
            let template = window.__erm_b64utf8(templateB64);

            // Build new nodes
            let temp = document.createElement('div');
            let html = "";
            items.forEach((item, index) => {
              window.__current_eval_id = anchorId;
              try {
                html += renderItem(item, index, template);
              } finally {
                window.__current_eval_id = null;
              }
            });
            temp.innerHTML = html;

            // Reconcile instead of innerHTML overwrite
            reconcileNodes(anchor, Array.from(temp.childNodes));
          }
        }
      }
    });
  };

  window.__erm_register_if = function (anchorId, getHtml) {
    window.__erm_bindings.push({
      id: anchorId,
      update: () => {
        let anchor = document.getElementById(anchorId);
        if (anchor) {
          let newHtml = "";
          try {
            window.__current_eval_id = anchorId;
            newHtml = getHtml();
          } catch (e) { }
          finally {
            window.__current_eval_id = null;
          }
          if (anchor.__erm_last !== newHtml) {
            anchor.__erm_last = newHtml;

            // Reconcile instead of innerHTML overwrite
            let temp = document.createElement('div');
            temp.innerHTML = newHtml;
            reconcileNodes(anchor, Array.from(temp.childNodes));
          }
        }
      }
    });
  };

  // Client-side Router / Navigation Interceptor
  async function navigate(path, push = true) {
    try {
      if (window.__hmr_data) {
        window.__hmr_data.states = {};
      }
      const res = await fetch(path);
      const html = await res.text();

      const parser = new DOMParser();
      const doc = parser.parseFromString(html, 'text/html');

      document.title = doc.title;

      // Update style block
      const newStyle = doc.getElementById('__erm_styles');
      const oldStyle = document.getElementById('__erm_styles');
      if (newStyle && oldStyle) {
        oldStyle.innerHTML = newStyle.innerHTML;
      } else if (newStyle) {
        document.head.appendChild(newStyle.cloneNode(true));
      }

      // Reconcile body children
      reconcileNodes(document.body, Array.from(doc.body.childNodes));

      initLoadingSwap();

      // Reset bindings and events for the new page
      window.__erm_bindings.forEach(b => {
        if (b && typeof b.cleanup === 'function') {
          try { b.cleanup(); } catch (e) { console.error("Effect cleanup failed on page navigate:", e); }
        }
      });
      window.__erm_bindings = [];
      window.__erm_bindings.push = function (binding) {
        return Array.prototype.push.call(this, binding);
      };
      window.__erm_events = [];
      window.__erm_dynamic_events = {};
      nextDynamicEventId = 0;

      // Execute page scripts
      const scripts = doc.querySelectorAll('script.__erm_script');
      scripts.forEach(script => {
        const newScript = document.createElement('script');
        newScript.className = '__erm_script';
        if (script.type) {
          newScript.type = script.type;
        }
        newScript.text = script.text;
        document.head.appendChild(newScript);
        newScript.remove();
      });

      if (push) {
        history.pushState(null, '', path);
      }

      _initReactivity();
      window.__erm_update();

      setTimeout(() => {
        _initReactivity();
        window.__erm_update();
      }, 50);
    } catch (err) {
      console.error("Navigation failed:", err);
      window.location.href = path;
    }
  }

  document.addEventListener('click', e => {
    const link = e.target.closest('a');
    if (link &&
      link.href &&
      !link.target &&
      !link.hasAttribute('download') &&
      new URL(link.href).origin === window.location.origin) {
      const targetPath = new URL(link.href).pathname;
      if (targetPath.startsWith('/api/')) return;

      e.preventDefault();
      navigate(targetPath);
    }
  });

  window.addEventListener('popstate', () => {
    navigate(window.location.pathname, false);
  });

  window.addEventListener('error', (event) => {
    if (typeof window.__erm_show_error_overlay === 'function') {
      const error = event.error || { message: event.message };
      const stack = error.stack || '';
      const filename = event.filename ? event.filename.replace(window.location.origin, '') : 'unknown';
      window.__erm_show_error_overlay({
        type: 'Runtime Error',
        file: filename + (event.lineno ? `:${event.lineno}:${event.colno}` : ''),
        title: error.name || 'Error',
        message: error.message || event.message,
        stack: stack
      });
    }
  });

  window.addEventListener('unhandledrejection', (event) => {
    if (typeof window.__erm_show_error_overlay === 'function') {
      const reason = event.reason || {};
      const stack = reason.stack || '';
      if (reason.message === "Compilation error overlay shown") {
        event.preventDefault();
        return;
      }
      window.__erm_show_error_overlay({
        type: 'Unhandled Rejection',
        file: 'Promise Rejection',
        title: reason.name || 'Error',
        message: reason.message || String(reason),
        stack: stack
      });
    }
  });

export { useState, useEffect, onMount, useParams, effect };
