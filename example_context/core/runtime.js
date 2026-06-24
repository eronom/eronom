(() => {
  window.__hmr_data = window.__hmr_data || { states: {} };
  if (!window.__hmr_data.states) window.__hmr_data.states = {};
  
  window.__erm_b64utf8 = function(str) {
    if (!str) return '';
    return decodeURIComponent(escape(window.atob(str)));
  };

  window.__erm_escape = function(val) {
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
      } catch(e) {
        console.error("Binding update failed:", e);
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
          } catch(e) {
            console.error("Delayed binding update failed:", e);
          } finally {
            popListener();
          }
        }
      }
    });
  }

  window.useState = function(val, name) {
    if (name && window.__hmr_data.states[name] !== undefined) {
      val = window.__hmr_data.states[name];
    }

    const subscribers = new Set();

    function notify() {
      subscribers.forEach(b => queueUpdate(b));
    }

    // Helper to proxy array mutations
    function makeArrayProxy(arr) {
      return new Proxy(arr, {
        get(target, prop) {
          let res = target[prop];
          if (typeof res === 'function') {
            const methods = ['push', 'pop', 'shift', 'unshift', 'splice', 'sort', 'reverse'];
            if (methods.includes(prop)) {
              return (...args) => {
                const result = target[prop].apply(target, args);
                notify();
                return result;
              };
            }
            return res.bind(target);
          }
          return res;
        },
        set(target, prop, newVal) {
          target[prop] = newVal;
          notify();
          return true;
        }
      });
    }

    if (typeof val === 'function') {
      const stateObj = {
        _getter: val,
        id: name,
        defaultValue: undefined,
        get value() {
          if (activeListener) {
            subscribers.add(activeListener);
            activeListener.deps = activeListener.deps || new Set();
            activeListener.deps.add(subscribers);
          }
          return this._getter();
        },
        toString() { return this.value; },
        valueOf() { return this.value; },
        [Symbol.toPrimitive]() { return this.value; }
      };
      if (name) {
        window[name] = stateObj;
      }
      return stateObj;
    }

    const container = {
      _val: val,
      id: name,
      defaultValue: val,
      toString() { return this._val; },
      valueOf() { return this._val; },
      [Symbol.toPrimitive]() { return this._val; }
    };

    const stateProxy = new Proxy(container, {
      get(target, prop) {
        if (prop === 'value') {
          if (activeListener) {
            subscribers.add(activeListener);
            activeListener.deps = activeListener.deps || new Set();
            activeListener.deps.add(subscribers);
          }
          if (Array.isArray(target._val)) {
            return makeArrayProxy(target._val);
          }
          return target._val;
        }
        if (prop === 'id') {
          return target.id;
        }
        if (prop === 'defaultValue') {
          return target.defaultValue;
        }
        let res = target[prop];
        if (Array.isArray(target._val) && typeof target._val[prop] === 'function') {
          const methods = ['push', 'pop', 'shift', 'unshift', 'splice', 'sort', 'reverse'];
          if (methods.includes(prop)) {
            return (...args) => {
              const result = target._val[prop].apply(target._val, args);
              notify();
              return result;
            };
          }
        }
        return res !== undefined ? res : target._val[prop];
      },
      set(target, prop, newVal) {
        if (prop === 'value') {
          if (target._val !== newVal) {
            target._val = newVal;
            if (name) window.__hmr_data.states[name] = newVal;
            notify();
          }
          return true;
        }
        target[prop] = newVal;
        return true;
      }
    });

    if (name) {
      window[name] = stateProxy;
    }
    return stateProxy;
  };

  window.useParams = function() { return window.__erm_params || {}; };

  window.__erm_bindings = [];
  window.__erm_bindings.push = function(binding) {
    return Array.prototype.push.call(this, binding);
  };

  window.__erm_events = [];
  window.__current_eval_id = null;

  window.__erm_update = function() {
    window.__erm_bindings.forEach(b => {
      if (!b.initialized) {
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
  window.__erm_render_template = function(template, evalFn) {
    return template.replace(/\{([^{}#/:][^{}]*)\}/g, (m, expr) => {
      try {
        let val = evalFn(expr);
        return val === undefined ? "" : val;
      } catch(e) { return ""; }
    });
  };

  window.__erm_register_for = function(anchorId, getCollection, templateB64, renderItem) {
    window.__erm_bindings.push({
      id: anchorId,
      update: () => {
        let anchor = document.getElementById(anchorId);
        if (anchor) {
          let items = [];
          try { items = getCollection(); } catch(e) {}
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

  window.__erm_register_if = function(anchorId, getHtml) {
    window.__erm_bindings.push({
      id: anchorId,
      update: () => {
        let anchor = document.getElementById(anchorId);
        if (anchor) {
          let newHtml = "";
          try {
            window.__current_eval_id = anchorId;
            newHtml = getHtml();
          } catch(e) {}
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
      
      // Reset bindings and events for the new page
      window.__erm_bindings = [];
      window.__erm_bindings.push = function(binding) {
        return Array.prototype.push.call(this, binding);
      };
      window.__erm_events = [];
      
      // Execute page scripts
      const scripts = doc.querySelectorAll('script.__erm_script');
      scripts.forEach(script => {
        const newScript = document.createElement('script');
        newScript.className = '__erm_script';
        newScript.text = script.text;
        document.head.appendChild(newScript);
        newScript.remove();
      });
      
      if (push) {
        history.pushState(null, '', path);
      }
      
      _initReactivity();
      window.__erm_update();
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
})();
