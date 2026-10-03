// Eronom Reactive Runtime - Fine-Grained Reactivity
// Pure reactive graph, dependency tracking, zero-polling DOM bindings

import { createHotContext, showError } from './hmr.js';

// --- HMR State Store ---
const hot = typeof import.meta !== 'undefined' && import.meta.hot
  ? import.meta.hot
  : createHotContext('/modules/erm/runtime.js');

const getHmrState = (name) => {
  return hot?.data?.states?.[name];
};

const setHmrState = (name, val) => {
  if (hot) {
    hot.data.states = hot.data.states || {};
    hot.data.states[name] = val;
  }
};

const clearHmrState = () => {
  if (hot?.data) {
    hot.data.states = {};
  }
};

export function escapeHtml(val) {
  if (val === null || val === undefined) return '';
  return String(val)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

// --- Reactive Graph Core ---
const equalFn = (a, b) => a === b;
const STALE = 1;

let Listener = null;
let Owner = null;
let Updates = null;
let Effects = null;
let ExecCount = 0;

export function getListener() {
  return Listener;
}

export function getOwner() {
  return Owner;
}

export function untrack(fn) {
  const prev = Listener;
  Listener = null;
  try {
    return fn();
  } finally {
    Listener = prev;
  }
}

export function onCleanup(fn) {
  if (Owner === null) return fn;
  if (Owner.cleanups === null) Owner.cleanups = [fn];
  else Owner.cleanups.push(fn);
  return fn;
}

function cleanNode(node) {
  let i;
  if (node.sources) {
    while (node.sources.length) {
      const source = node.sources.pop();
      const index = node.sourceSlots.pop();
      const obs = source.observers;
      if (obs && obs.length) {
        const n = obs.pop();
        const s = source.observerSlots.pop();
        if (index < obs.length) {
          n.sourceSlots[s] = index;
          obs[index] = n;
          source.observerSlots[index] = s;
        }
      }
    }
  }
  if (node.owned) {
    for (i = 0; i < node.owned.length; i++) cleanNode(node.owned[i]);
    node.owned = null;
  }
  if (node.cleanups) {
    for (i = 0; i < node.cleanups.length; i++) {
      try {
        node.cleanups[i]();
      } catch (err) {
        handleError(err);
      }
    }
    node.cleanups = null;
  }
  node.state = 0;
}

function handleError(err) {
  console.error("[Reactivity Error]", err);
  showError({
    type: 'Reactivity Error',
    file: 'runtime.js',
    title: err?.name || 'Error',
    message: err?.message || String(err),
    stack: err?.stack || ''
  });
}

function runUpdates(fn, init) {
  if (Updates) return fn();
  let wait = false;
  if (!init) Updates = [];
  if (Effects) wait = true;
  else Effects = [];
  ExecCount++;
  try {
    const res = fn();
    completeUpdates(wait);
    return res;
  } catch (err) {
    if (!wait) Effects = null;
    Updates = null;
    handleError(err);
  }
}

function completeUpdates(wait) {
  if (Updates) {
    const q = Updates;
    Updates = null;
    for (let i = 0; i < q.length; i++) {
      if (q[i].state) updateComputation(q[i]);
    }
  }
  if (wait) return;
  const resEffects = Effects;
  Effects = null;
  if (resEffects && resEffects.length) {
    runUpdates(() => {
      for (let i = 0; i < resEffects.length; i++) {
        if (resEffects[i].state) updateComputation(resEffects[i]);
      }
    }, false);
  }
}

function runComputation(node, value, time) {
  let nextValue;
  const owner = Owner;
  const listener = Listener;
  Listener = Owner = node;
  try {
    nextValue = node.fn(value);
  } catch (err) {
    node.state = STALE;
    if (node.owned) node.owned.forEach(cleanNode);
    node.owned = null;
    node.updatedAt = time + 1;
    return handleError(err);
  } finally {
    Listener = listener;
    Owner = owner;
  }
  if (!node.updatedAt || node.updatedAt <= time) {
    if (node.updatedAt != null && 'observers' in node) {
      writeSignal(node, nextValue, true);
    } else {
      node.value = nextValue;
    }
    node.updatedAt = time;
  }
}

function updateComputation(node) {
  if (!node.fn) return;
  cleanNode(node);
  const time = ExecCount;
  runComputation(node, node.value, time);
}

function createComputation(fn, init, pure, state = STALE, options) {
  const c = {
    fn,
    state,
    updatedAt: null,
    owned: null,
    sources: null,
    sourceSlots: null,
    cleanups: null,
    value: init,
    owner: Owner,
    context: Owner ? Owner.context : null,
    pure,
    name: options?.name
  };

  if (Owner) {
    if (!Owner.owned) Owner.owned = [c];
    else Owner.owned.push(c);
  }

  return c;
}

export function readSignal() {
  if (this.sources && this.state) {
    const updates = Updates;
    Updates = null;
    runUpdates(() => updateComputation(this), false);
    Updates = updates;
  }
  if (Listener) {
    const sSlot = this.observers ? this.observers.length : 0;
    if (!Listener.sources) {
      Listener.sources = [this];
      Listener.sourceSlots = [sSlot];
    } else {
      Listener.sources.push(this);
      Listener.sourceSlots.push(sSlot);
    }
    if (!this.observers) {
      this.observers = [Listener];
      this.observerSlots = [Listener.sources.length - 1];
    } else {
      this.observers.push(Listener);
      this.observerSlots.push(Listener.sources.length - 1);
    }
  }
  return this.value;
}

export function writeSignal(node, value, isComp) {
  let current = node.value;
  if (!node.comparator || !node.comparator(current, value)) {
    node.value = value;
    if (node.observers && node.observers.length) {
      runUpdates(() => {
        for (let i = 0; i < node.observers.length; i++) {
          const o = node.observers[i];
          if (!o.state) {
            if (o.pure) Updates.push(o);
            else Effects.push(o);
          }
          o.state = STALE;
        }
      }, false);
    }
  }
  return value;
}

export function createSignal(value, options) {
  const comparator = options?.equals === false ? () => false : (options?.equals || equalFn);
  const s = {
    value,
    observers: null,
    observerSlots: null,
    comparator,
    name: options?.name
  };

  const setter = (next) => {
    if (typeof next === 'function') {
      next = next(s.value);
    }
    return writeSignal(s, next);
  };

  return [readSignal.bind(s), setter];
}

export function createRoot(fn, detachedOwner) {
  const listener = Listener;
  const owner = Owner;
  const root = {
    owned: null,
    cleanups: null,
    context: detachedOwner !== undefined ? detachedOwner?.context : (owner ? owner.context : null),
    owner: detachedOwner !== undefined ? detachedOwner : owner
  };
  Owner = root;
  Listener = null;
  try {
    return runUpdates(() => fn(() => cleanNode(root)), true);
  } finally {
    Owner = owner;
    Listener = listener;
  }
}

export function createEffect(fn, value, options) {
  const c = createComputation(fn, value, false, STALE, options);
  if (Effects) Effects.push(c);
  else updateComputation(c);
}

export function createRenderEffect(fn, value, options) {
  const c = createComputation(fn, value, false, STALE, options);
  updateComputation(c);
}

export function createMemo(fn, value, options) {
  const c = createComputation(fn, value, true, 0, options);
  c.observers = null;
  c.observerSlots = null;
  c.comparator = options?.equals === false ? () => false : (options?.equals || equalFn);
  updateComputation(c);
  return readSignal.bind(c);
}

export function onMount(fn) {
  createEffect(() => untrack(fn));
}

export function batch(fn) {
  return runUpdates(fn, false);
}

// --- Keyed Array Reconciliation (mapArray) ---
const FALLBACK = Symbol("fallback");

export function mapArray(listAccessor, mapFn, options = {}) {
  let items = [];
  let mapped = [];
  let disposers = [];
  let len = 0;
  let indexes = mapFn.length > 1 ? [] : null;

  onCleanup(() => {
    for (let i = 0; i < disposers.length; i++) disposers[i]();
  });

  return () => {
    let newItems = listAccessor() || [];
    let newLen = newItems.length;

    return untrack(() => {
      let newIndices;
      let newIndicesNext;
      let temp;
      let tempdisposers;
      let tempIndexes;
      let start = 0;
      let end = 0;
      let newEnd = 0;
      let item;

      if (newLen === 0) {
        if (len !== 0) {
          for (let i = 0; i < disposers.length; i++) disposers[i]();
          disposers = [];
          items = [];
          mapped = [];
          len = 0;
          if (indexes) indexes = [];
        }
        if (options.fallback) {
          items = [FALLBACK];
          mapped[0] = createRoot(disposer => {
            disposers[0] = disposer;
            return options.fallback();
          });
          len = 1;
        }
      } else if (len === 0) {
        mapped = new Array(newLen);
        for (let j = 0; j < newLen; j++) {
          items[j] = newItems[j];
          mapped[j] = createRoot(mapper.bind(null, j, newItems[j]));
        }
        len = newLen;
      } else {
        temp = new Array(newLen);
        tempdisposers = new Array(newLen);
        if (indexes) tempIndexes = new Array(newLen);

        // skip common prefix
        for (
          start = 0, end = Math.min(len, newLen);
          start < end && items[start] === newItems[start];
          start++
        );

        // common suffix
        for (
          end = len - 1, newEnd = newLen - 1;
          end >= start && newEnd >= start && items[end] === newItems[newEnd];
          end--, newEnd--
        ) {
          temp[newEnd] = mapped[end];
          tempdisposers[newEnd] = disposers[end];
          if (indexes) tempIndexes[newEnd] = indexes[end];
        }

        newIndices = new Map();
        newIndicesNext = new Array(newEnd + 1);
        for (let j = newEnd; j >= start; j--) {
          item = newItems[j];
          let i = newIndices.get(item);
          newIndicesNext[j] = i === undefined ? -1 : i;
          newIndices.set(item, j);
        }

        for (let i = start; i <= end; i++) {
          item = items[i];
          let j = newIndices.get(item);
          if (j !== undefined && j !== -1) {
            temp[j] = mapped[i];
            tempdisposers[j] = disposers[i];
            if (indexes) tempIndexes[j] = indexes[i];
            j = newIndicesNext[j];
            newIndices.set(item, j);
          } else {
            disposers[i]();
          }
        }

        for (let j = start; j < newLen; j++) {
          if (j in temp) {
            mapped[j] = temp[j];
            disposers[j] = tempdisposers[j];
            if (indexes) {
              indexes[j] = tempIndexes[j];
              indexes[j](j);
            }
          } else {
            mapped[j] = createRoot(mapper.bind(null, j, newItems[j]));
          }
        }

        mapped = mapped.slice(0, (len = newLen));
        items = newItems.slice(0);
      }
      return mapped;
    });
  };

  function mapper(j, item, disposer) {
    disposers[j] = disposer;
    if (indexes) {
      const [s, set] = createSignal(j);
      indexes[j] = set;
      return mapFn(item, s);
    }
    return mapFn(item);
  }
}

// --- Flow Components: Show & For ---
export function Show(props) {
  const condition = createMemo(() => props.when);
  return createMemo(() => {
    const c = condition();
    if (c) {
      const ch = props.children;
      return typeof ch === 'function' ? ch(c) : ch;
    }
    return props.fallback ?? null;
  });
}

export function For(props) {
  const fallback = 'fallback' in props ? { fallback: () => props.fallback } : undefined;
  return createMemo(mapArray(() => props.each, props.children, fallback));
}

// --- High-Performance Fine-Grained DOM Bindings
export function bindText(target, accessor) {
  createRenderEffect(() => {
    const el = typeof target === 'string' ? document.getElementById(target) : target;
    if (!el) return;
    const val = accessor();
    const str = val === null || val === undefined ? '' : String(val);
    if (el.textContent !== str) {
      el.textContent = str;
    }
  });
}

export function bindEvent(target, eventName, handler) {
  const el = typeof target === 'string' ? document.getElementById(target) : target;
  if (!el) return;
  const listener = (event) => {
    batch(() => {
      handler(event);
    });
  };
  el.addEventListener(eventName, listener);
  onCleanup(() => {
    el.removeEventListener(eventName, listener);
  });
}

export function bindAttr(target, attrName, accessor) {
  createRenderEffect(() => {
    const el = typeof target === 'string' ? document.getElementById(target) : target;
    if (!el) return;
    const val = accessor();
    if (attrName === 'value') {
      if (el.value !== (val ?? '')) el.value = val ?? '';
    } else if (attrName === 'checked') {
      el.checked = Boolean(val);
    } else if (val === false || val === null || val === undefined) {
      el.removeAttribute(attrName);
    } else {
      el.setAttribute(attrName, val === true ? '' : String(val));
    }
  });
}

export function bindProvider(target, contextVar, accessor) {
  createRenderEffect(() => {
    const el = typeof target === 'string' ? document.getElementById(target) : target;
    if (!el) return;
    if (!el.__erm_providers) el.__erm_providers = {};
    el.__erm_providers[contextVar.id || contextVar] = accessor();
  });
}

export function renderIf(anchorId, branchesFn) {
  createRenderEffect(() => {
    const anchor = document.getElementById(anchorId);
    if (!anchor) return;
    let newHtml = '';
    try {
      newHtml = branchesFn();
    } catch (e) {
      console.error("[renderIf error]", e);
    }
    if (anchor.__erm_last_html !== newHtml) {
      anchor.__erm_last_html = newHtml;
      // Reconcile or update innerHTML
      const temp = document.createElement('div');
      temp.innerHTML = newHtml;
      reconcileNodes(anchor, Array.from(temp.childNodes));
    }
  });
}

export function renderFor(anchorId, getCollection, renderItem) {
  createRenderEffect(() => {
    const anchor = document.getElementById(anchorId);
    if (!anchor) return;
    let items = [];
    try {
      items = getCollection();
    } catch (e) { }
    if (!Array.isArray(items)) items = [];
    const itemsJson = JSON.stringify(items);
    if (anchor.__erm_last_items !== itemsJson) {
      anchor.__erm_last_items = itemsJson;
      const temp = document.createElement('div');
      let html = '';
      items.forEach((item, index) => {
        try {
          html += renderItem(item, index);
        } catch (e) {
          console.error("[renderFor item error]", e);
        }
      });
      temp.innerHTML = html;
      reconcileNodes(anchor, Array.from(temp.childNodes));
    }
  });
}

let nextDynamicEventId = 0;
const dynamicEvents = {};
export function registerEvent(event, handler) {
  const id = ++nextDynamicEventId;
  dynamicEvents[id] = { event, handler };
  return `data-erm-evt-id="${id}"`;
}

// DOM Reconciliation / Diffing Helper for HTML chunks
function reconcileNodes(parent, newNodes) {
  // Filter newNodes to exclude script tags and error overlays
  const cleanNewNodes = newNodes.filter(n => {
    if (n.nodeType === Node.ELEMENT_NODE) {
      const tag = n.tagName.toUpperCase();
      return tag !== 'SCRIPT' && tag !== 'ERM-ERROR-OVERLAY';
    }
    return true;
  });

  // Filter parent childNodes to match content nodes
  const childNodes = Array.from(parent.childNodes).filter(n => {
    if (n.nodeType === Node.ELEMENT_NODE) {
      const tag = n.tagName.toUpperCase();
      return tag !== 'SCRIPT' && tag !== 'ERM-ERROR-OVERLAY';
    }
    return true;
  });

  for (let k = cleanNewNodes.length; k < childNodes.length; k++) {
    if (childNodes[k] && childNodes[k].parentNode === parent) {
      parent.removeChild(childNodes[k]);
    }
  }

  for (let k = 0; k < cleanNewNodes.length; k++) {
    let existing = childNodes[k];
    let incoming = cleanNewNodes[k];
    if (!existing) {
      parent.appendChild(incoming);
    } else if (existing.nodeType !== incoming.nodeType) {
      parent.replaceChild(incoming, existing);
    } else if (existing.nodeType === Node.TEXT_NODE) {
      if (existing.nodeValue !== incoming.nodeValue) {
        existing.nodeValue = incoming.nodeValue;
      }
    } else if (existing.nodeType === Node.ELEMENT_NODE) {
      if (existing.tagName !== incoming.tagName) {
        parent.replaceChild(incoming, existing);
      } else {
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
        if (existing.tagName === 'INPUT' || existing.tagName === 'TEXTAREA') {
          let incomingVal = incoming.value;
          if (existing.tagName === 'TEXTAREA' && incoming.hasAttribute('value')) {
            incomingVal = incoming.getAttribute('value');
          }
          if (document.activeElement !== existing && existing.value !== incomingVal) {
            existing.value = incomingVal;
          }
          if (existing.checked !== incoming.checked) {
            existing.checked = incoming.checked;
          }
        }
        reconcileNodes(existing, Array.from(incoming.childNodes));
      }
    }
  }

  // Bind dynamic event attributes inside reconciled nodes
  parent.querySelectorAll('[data-erm-evt-id]').forEach(el => {
    const id = el.getAttribute('data-erm-evt-id');
    if (el.__erm_evt_id !== id) {
      if (el.__erm_evt_listener && el.__erm_evt_type) {
        el.removeEventListener(el.__erm_evt_type, el.__erm_evt_listener);
      }
      const ev = dynamicEvents[id];
      if (ev) {
        const wrapper = (event) => batch(() => ev.handler(event));
        el.addEventListener(ev.event, wrapper);
        el.__erm_evt_id = id;
        el.__erm_evt_type = ev.event;
        el.__erm_evt_listener = wrapper;
      }
    }
  });
}

// --- Backwards-Compatible useState & Signals Wrapper ---
const statesRegistry = new Map();

function createArrayProxy(arr, setter, name) {
  return new Proxy(arr, {
    get(target, prop) {
      const res = target[prop];
      if (typeof res === 'function') {
        const mutators = ['push', 'pop', 'shift', 'unshift', 'splice', 'sort', 'reverse'];
        if (mutators.includes(prop)) {
          return (...args) => {
            const ret = target[prop].apply(target, args);
            if (name) {
              setHmrState(name, target);
            }
            setter([...target]);
            return ret;
          };
        }
        return res.bind(target);
      }
      return res;
    },
    set(target, prop, newVal) {
      target[prop] = newVal;
      if (name) {
        setHmrState(name, target);
      }
      setter([...target]);
      return true;
    }
  });
}

export function useState(val, name) {
  if (name && statesRegistry.has(name)) {
    return statesRegistry.get(name);
  }
  const hmrVal = name ? getHmrState(name) : undefined;
  if (hmrVal !== undefined) {
    val = hmrVal;
  }

  if (typeof val === 'function') {
    const memo = createMemo(val, undefined, { name });
    const wrapper = function () { return memo(); };
    Object.defineProperty(wrapper, 'value', {
      get() { return memo(); },
      enumerable: true
    });
    wrapper.toString = () => String(memo());
    wrapper.valueOf = () => memo();
    wrapper[Symbol.toPrimitive] = () => memo();
    if (name) statesRegistry.set(name, wrapper);
    return wrapper;
  }

  const [get, set] = createSignal(val, { name });

  function signalWrapper(...args) {
    if (args.length > 0) return set(args[0]);
    return get();
  }

  Object.defineProperty(signalWrapper, 'value', {
    get() {
      const current = get();
      if (Array.isArray(current)) {
        return createArrayProxy(current, set, name);
      }
      return current;
    },
    set(newVal) {
      if (name) {
        setHmrState(name, newVal);
      }
      set(newVal);
    },
    enumerable: true
  });

  signalWrapper.toString = () => String(get());
  signalWrapper.valueOf = () => get();
  signalWrapper[Symbol.toPrimitive] = () => get();

  if (name) {
    statesRegistry.set(name, signalWrapper);
  }

  return signalWrapper;
}

export function useEffect(callback, depsFn) {
  let lastDeps;
  let hasRun = false;
  let cleanup;

  createEffect(() => {
    if (typeof cleanup === 'function') {
      try { cleanup(); } catch (e) { console.error("Effect cleanup failed:", e); }
      cleanup = null;
    }

    let shouldRun = false;
    let currentDeps;

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
        untrack(() => {
          cleanup = callback();
        });
      } else {
        cleanup = callback();
      }
    }
  });

  onCleanup(() => {
    if (typeof cleanup === 'function') {
      try { cleanup(); } catch (e) { console.error("Effect cleanup failed:", e); }
      cleanup = null;
    }
  });
}

export const effect = createEffect;

let currentParams = {};
export function setParams(p) { currentParams = p; }
export function useParams() { return currentParams; }

// --- Suspense & Loading Swap ---
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

  const originalFetch = globalThis.fetch;
  let activeInitFetches = 0;
  let finished = false;

  function checkLoadingFinished() {
    if (finished) return;
    setTimeout(() => {
      if (activeInitFetches <= 0) {
        finished = true;
        globalThis.fetch = originalFetch;
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
      }
    }, 20);
  }

  globalThis.fetch = function (...args) {
    if (finished) return originalFetch(...args);
    activeInitFetches++;
    return originalFetch(...args).finally(() => {
      activeInitFetches--;
      queueMicrotask(checkLoadingFinished);
    });
  };

  setTimeout(() => {
    checkLoadingFinished();
  }, 300);
}

initLoadingSwap();

// --- Client-Side Router / Navigation Interceptor ---
let currentPageRootDispose = null;
export function setCurrentPageDispose(dispose) {
  currentPageRootDispose = dispose;
}

async function navigate(path, push = true) {
  try {
    clearHmrState();
    const res = await fetch(path);
    const html = await res.text();

    const parser = new DOMParser();
    const doc = parser.parseFromString(html, 'text/html');

    document.title = doc.title;

    // Dispose old page root to prevent memory leaks and clean up all subscriptions
    if (typeof currentPageRootDispose === 'function') {
      try { currentPageRootDispose(); } catch (e) { console.error("Page dispose failed:", e); }
      currentPageRootDispose = null;
    }
    statesRegistry.clear();

    // Update style blocks
    const newStyle = doc.getElementById('__erm_styles');
    const oldStyle = document.getElementById('__erm_styles');
    if (newStyle && oldStyle) {
      if (newStyle.tagName === 'LINK') {
        if (oldStyle.getAttribute('href') !== newStyle.getAttribute('href')) {
          oldStyle.setAttribute('href', newStyle.getAttribute('href') || '');
        }
      } else {
        oldStyle.innerHTML = newStyle.innerHTML;
      }
    } else if (newStyle) {
      document.head.appendChild(newStyle.cloneNode(true));
    } else if (oldStyle) {
      oldStyle.remove();
    }

    const newScopedStyle = doc.getElementById('__erm_scoped_styles');
    const oldScopedStyle = document.getElementById('__erm_scoped_styles');
    if (newScopedStyle && oldScopedStyle) {
      oldScopedStyle.innerHTML = newScopedStyle.innerHTML;
    } else if (newScopedStyle) {
      document.head.appendChild(newScopedStyle.cloneNode(true));
    } else if (oldScopedStyle) {
      oldScopedStyle.remove();
    }

    // Collect page scripts before reconciling body
    const scriptElements = Array.from(doc.querySelectorAll('script.__erm_script'));
    const scriptData = scriptElements.map(s => ({
      type: s.type,
      text: s.textContent || s.text || ''
    }));
    scriptElements.forEach(s => s.remove());

    reconcileNodes(document.body, Array.from(doc.body.childNodes));
    initLoadingSwap();

    // Clean up previous dynamically injected page scripts
    document.querySelectorAll('head script.__erm_script').forEach(s => s.remove());

    // Execute new page scripts
    for (const item of scriptData) {
      if (item.type === 'module') {
        const origin = location.origin;
        const moduleCode = item.text
          .replace(/\bfrom\s+(['"])(\/[^'"]+)\1/g, `from "${origin}$2"`)
          .replace(/\bimport\s+(['"])(\/[^'"]+)\1/g, `import "${origin}$2"`)
          .replace(/\bimport\s*\(\s*(['"])(\/[^'"]+)\1\s*\)/g, `import("${origin}$2")`);
        const blob = new Blob([moduleCode], { type: 'application/javascript' });
        const url = URL.createObjectURL(blob);
        try {
          await import(url);
        } catch (err) {
          console.error("[ERM navigation] Page module script failed:", err);
        } finally {
          URL.revokeObjectURL(url);
        }
      } else {
        const newScript = document.createElement('script');
        newScript.className = '__erm_script';
        if (item.type) newScript.type = item.type;
        newScript.textContent = item.text;
        document.head.appendChild(newScript);
      }
    }

    if (push) {
      history.pushState(null, '', path);
    }
  } catch (err) {
    console.error("Navigation failed:", err);
    location.href = path;
  }
}

document.addEventListener('click', e => {
  const link = e.target.closest('a');
  if (link &&
    link.href &&
    !link.target &&
    !link.hasAttribute('download') &&
    new URL(link.href).origin === location.origin) {
    const targetPath = new URL(link.href).pathname;
    if (targetPath.startsWith('/api/')) return;
    e.preventDefault();
    navigate(targetPath);
  }
});

addEventListener('popstate', () => {
  navigate(location.pathname, false);
});

// --- In-Place ERM Hot Module Replacement (HMR) ---
export async function applyErmHmr(targetPath, timestamp) {
  try {
    const url = new URL(location.href);
    url.searchParams.set('__erm_hmr_t', String(timestamp || Date.now()));

    const res = await fetch(url.href, {
      headers: {
        'Accept': 'text/html',
        'X-ERM-HMR': '1'
      }
    });

    if (!res.ok) {
      const errText = await res.text();
      const doc = new DOMParser().parseFromString(errText, 'text/html');
      const errFile = doc.body?.getAttribute('data-compile-error-file') || targetPath;
      const errMsg = doc.body?.getAttribute('data-compile-error-message') || `Server error (${res.status})`;
      showError({
        type: 'Build Error',
        file: errFile,
        message: errMsg,
        id: errFile
      });
      return;
    }

    if (typeof window !== 'undefined' && window.hmrClient && typeof window.hmrClient.clearError === 'function') {
      window.hmrClient.clearError();
    }

    const html = await res.text();
    const doc = new DOMParser().parseFromString(html, 'text/html');

    // 1. Update Title
    if (doc.title && document.title !== doc.title) {
      document.title = doc.title;
    }

    // 2. Update Style blocks in-place
    const newScopedStyle = doc.getElementById('__erm_scoped_styles');
    const oldScopedStyle = document.getElementById('__erm_scoped_styles');
    if (newScopedStyle && oldScopedStyle) {
      oldScopedStyle.textContent = newScopedStyle.textContent;
    } else if (newScopedStyle) {
      document.head.appendChild(newScopedStyle.cloneNode(true));
    } else if (oldScopedStyle) {
      oldScopedStyle.remove();
    }

    const newStyle = doc.getElementById('__erm_styles');
    const oldStyle = document.getElementById('__erm_styles');
    if (newStyle && oldStyle) {
      if (newStyle.tagName === 'LINK') {
        const newHref = newStyle.getAttribute('href');
        if (newHref && oldStyle.getAttribute('href') !== newHref) {
          oldStyle.setAttribute('href', newHref);
        }
      } else {
        oldStyle.textContent = newStyle.textContent;
      }
    } else if (newStyle) {
      document.head.appendChild(newStyle.cloneNode(true));
    } else if (oldStyle) {
      oldStyle.remove();
    }

    // 3. Dispose old page reactive root (unsubscribes listeners & cleanups)
    if (typeof currentPageRootDispose === 'function') {
      try {
        currentPageRootDispose();
      } catch (e) {
        console.error('[eronom] [hmr] Error disposing previous root:', e);
      }
      currentPageRootDispose = null;
    }

    // Notice: We intentionally do NOT clear statesRegistry!
    // Active signals and state are preserved across HMR!

    // 4. Collect incoming page scripts
    const scriptElements = Array.from(doc.querySelectorAll('script.__erm_script'));
    const scriptData = scriptElements.map(s => ({
      type: s.type,
      text: s.textContent || s.text || ''
    }));
    // Remove all scripts from doc so reconcileNodes only touches DOM content
    doc.querySelectorAll('script').forEach(s => s.remove());

    // Clean up previous dynamically injected page scripts in current head/body
    document.querySelectorAll('head script.__erm_script').forEach(s => s.remove());
    document.querySelectorAll('body script.__erm_script').forEach(s => s.remove());

    // 5. Reconcile DOM in-place without page reload
    reconcileNodes(document.body, Array.from(doc.body.childNodes));
    initLoadingSwap();

    // 6. Execute updated page scripts
    for (const item of scriptData) {
      if (item.type === 'module') {
        const origin = location.origin;
        const moduleCode = item.text
          .replace(/\bfrom\s+(['"])(\/[^'"]+)\1/g, `from "${origin}$2"`)
          .replace(/\bimport\s+(['"])(\/[^'"]+)\1/g, `import "${origin}$2"`)
          .replace(/\bimport\s*\(\s*(['"])(\/[^'"]+)\1\s*\)/g, `import("${origin}$2")`);
        const blob = new Blob([moduleCode], { type: 'application/javascript' });
        const url = URL.createObjectURL(blob);
        try {
          await import(url);
        } catch (err) {
          console.error("[eronom] [hmr] Page module script failed:", err);
          showError({
            type: 'Runtime Error',
            file: targetPath,
            message: err.message,
            stack: err.stack
          });
        } finally {
          URL.revokeObjectURL(url);
        }
      } else {
        const newScript = document.createElement('script');
        newScript.className = '__erm_script';
        if (item.type) newScript.type = item.type;
        newScript.textContent = item.text;
        document.head.appendChild(newScript);
      }
    }

    console.log(`[eronom] [hmr] ${targetPath}`);
  } catch (err) {
    console.error('[eronom] [hmr] Update error:', err);
    location.reload();
  }
}

if (typeof window !== 'undefined') {
  window.__erm_apply_hmr = applyErmHmr;
}


