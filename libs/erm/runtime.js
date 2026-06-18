(() => {
  window.__hmr_data = window.__hmr_data || { states: {} };
  if (!window.__hmr_data.states) window.__hmr_data.states = {};
  window.__erm_b64utf8 = function(str) {
      return decodeURIComponent(escape(window.atob(str)));
    };
  window.useState = function(val, name) {
    if (name && window.__hmr_data.states[name] !== undefined) {
      val = window.__hmr_data.states[name];
    }
    if (typeof val === 'function') {
      return {
        _getter: val,
        get value() { return this._getter(); },
        toString() { return this.value; },
        valueOf() { return this.value; },
        [Symbol.toPrimitive]() { return this.value; }
      };
    }
    const container = { 
      _val: val,
      toString() { return this._val; },
      valueOf() { return this._val; },
      [Symbol.toPrimitive]() { return this._val; }
    };
    return new Proxy(container, {
      get(target, prop) {
        if (prop === 'value') return target._val;
        let res = target[prop];
        if (Array.isArray(target._val) && typeof target._val[prop] === 'function') {
           const methods = ['push', 'pop', 'shift', 'unshift', 'splice', 'sort', 'reverse'];
           if (methods.includes(prop)) {
             return (...args) => {
               const result = target._val[prop].apply(target._val, args);
               if (window.__erm_update) window.__erm_update();
               return result;
             };
           }
        }
        return res !== undefined ? res : target._val[prop];
      },
      set(target, prop, newVal) {
        if (prop === 'value') {
          target._val = newVal;
          if (name) window.__hmr_data.states[name] = newVal;
          if (window.__erm_update) window.__erm_update();
          return true;
        }
        target[prop] = newVal;
        return true;
      }
    });
  };
  window.useParams = function() { return window.__erm_params || {}; };
  window.__erm_bindings = [];
  window.__erm_events = [];
  let _updateQueued = false;
  window.__erm_update = function() {
    if (_updateQueued) return;
    _updateQueued = true;
    requestAnimationFrame(() => {
      window.__erm_bindings.forEach(b => {
        try {
          if (typeof b.update === 'function') { b.update(); } 
          else {
            let val = b.get();
            if (b.last !== val) { b.last = val; let el = document.getElementById(b.id); if (el) el.innerText = val === undefined ? '' : val; }
          }
        } catch(e) {}
      });
      if (typeof _initReactivity === 'function') _initReactivity();
      _updateQueued = false;
    });
  };
  function _initReactivity() {
    window.__erm_events.forEach(ev => {
      let el = document.getElementById(ev.id);
      if (el && !el.__erm_listener_added) { el.addEventListener(ev.event, ev.handler); el.__erm_listener_added = true; }
    });
    window.__erm_update();
  }
  if (document.readyState === 'loading') { document.addEventListener('DOMContentLoaded', _initReactivity); } 
  else { _initReactivity(); }
  setTimeout(_initReactivity, 10);

  // Helper functions for template rendering, conditional branches, and loops
  window.__erm_render_template = function(template, evalFn) {
    return template.replace(/\{{([^{}#/:][^{}]*)\}}/g, (m, expr) => {
      try { 
        let val = evalFn(expr); 
        return val === undefined ? "" : val;
      } catch(e) { return ""; }
    });
  };

  window.__erm_register_for = function(anchorId, getCollection, templateB64, renderItem) {
    window.__erm_bindings.push({
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
            let html = "";
            items.forEach((item, index) => {
              html += renderItem(item, index, template);
            });
            anchor.innerHTML = html;
          }
        }
      }
    });
  };

  window.__erm_register_if = function(anchorId, getHtml) {
    window.__erm_bindings.push({
      update: () => {
        let anchor = document.getElementById(anchorId);
        if (anchor) {
          let newHtml = "";
          try { newHtml = getHtml(); } catch(e) {}
          if (anchor.__erm_last !== newHtml) {
            anchor.__erm_last = newHtml;
            anchor.innerHTML = newHtml;
            if (window.__erm_update) window.__erm_update();
          }
        }
      }
    });
  };
})();
