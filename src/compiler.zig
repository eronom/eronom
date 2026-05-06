const std = @import("std");
const eval = @import("eval.zig");

pub fn scopeCSS(allocator: std.mem.Allocator, css: []const u8, scopeID: []const u8) ![]const u8 {
    var result: std.ArrayList(u8) = .empty;
    var i: usize = 0;
    while (i < css.len) {
        const brace_idx = std.mem.indexOfScalarPos(u8, css, i, '{') orelse break;
        const selector = css[i..brace_idx];
        const block_end = std.mem.indexOfScalarPos(u8, css, brace_idx, '}') orelse break;
        const block = css[brace_idx .. block_end + 1];

        var sel_it = std.mem.tokenizeScalar(u8, selector, ',');
        var first = true;
        while (sel_it.next()) |s| {
            if (!first) try result.appendSlice(allocator, ", ");
            const trimmed = std.mem.trim(u8, s, " \t\n\r");
            if (trimmed.len > 0) {
                if (std.mem.containsAtLeast(u8, trimmed, 1, "%") or 
                    (std.mem.eql(u8, trimmed, "to")) or 
                    (std.mem.eql(u8, trimmed, "from")) or 
                    (std.mem.startsWith(u8, trimmed, "body")) or 
                    (std.mem.startsWith(u8, trimmed, "html"))) 
                {
                    try result.appendSlice(allocator, trimmed);
                } else {
                    const colon_idx = std.mem.indexOfScalar(u8, trimmed, ':');
                    if (colon_idx) |idx| {
                        try result.appendSlice(allocator, trimmed[0..idx]);
                        try result.appendSlice(allocator, "[");
                        try result.appendSlice(allocator, scopeID);
                        try result.appendSlice(allocator, "]");
                        try result.appendSlice(allocator, trimmed[idx..]);
                    } else {
                        try result.appendSlice(allocator, trimmed);
                        try result.appendSlice(allocator, "[");
                        try result.appendSlice(allocator, scopeID);
                        try result.appendSlice(allocator, "]");
                    }
                }
            }
            first = false;
        }
        try result.appendSlice(allocator, " ");
        try result.appendSlice(allocator, block);
        i = block_end + 1;
    }
    if (i < css.len) try result.appendSlice(allocator, css[i..]);
    return result.toOwnedSlice(allocator);
}

pub fn scopeHTML(allocator: std.mem.Allocator, html: []const u8, scopeID: []const u8) ![]const u8 {
    var result: std.ArrayList(u8) = .empty;
    var i: usize = 0;
    while (i < html.len) {
        const tag_start = std.mem.indexOfScalarPos(u8, html, i, '<') orelse {
            try result.appendSlice(allocator, html[i..]);
            break;
        };
        try result.appendSlice(allocator, html[i..tag_start]);
        
        const tag_end = std.mem.indexOfScalarPos(u8, html, tag_start, '>') orelse {
            try result.appendSlice(allocator, html[tag_start..]);
            break;
        };

        const tag_content = html[tag_start + 1 .. tag_end];
        if (tag_content.len > 0 and tag_content[0] != '/') {
            var parts_it = std.mem.tokenizeAny(u8, tag_content, " \t\n\r");
            const tag_name = parts_it.next() orelse "";
            
            const is_component = tag_name.len > 0 and std.ascii.isUpper(tag_name[0]);
            const is_global = std.mem.eql(u8, tag_name, "html") or std.mem.eql(u8, tag_name, "head") or std.mem.eql(u8, tag_name, "body") or std.mem.eql(u8, tag_name, "!DOCTYPE") or std.mem.eql(u8, tag_name, "script") or std.mem.eql(u8, tag_name, "style");

            if (!is_component and !is_global) {
                try result.append(allocator, '<');
                try result.appendSlice(allocator, tag_name);
                try result.append(allocator, ' ');
                try result.appendSlice(allocator, scopeID);
                try result.appendSlice(allocator, tag_content[tag_name.len..]);
                try result.append(allocator, '>');
            } else {
                try result.append(allocator, '<');
                try result.appendSlice(allocator, tag_content);
                try result.append(allocator, '>');
            }
        } else {
            try result.append(allocator, '<');
            try result.appendSlice(allocator, tag_content);
            try result.append(allocator, '>');
        }
        i = tag_end + 1;
    }
    return result.toOwnedSlice(allocator);
}

pub const ProcessResult = struct {
    html: []const u8,
    scripts: std.ArrayList([]const u8),
    styles: std.ArrayList([]const u8),
    signal_vars: std.StringHashMap([]const u8),
};

fn parseReactivity(allocator: std.mem.Allocator, html: []const u8, bindings: *std.ArrayList([]const u8), events: *std.ArrayList([]const u8), signals: []const []const u8) ![]const u8 {
    var out: std.ArrayList(u8) = .empty;
    var i: usize = 0;
    var in_tag = false;
    
    while (i < html.len) {
        const c = html[i];
        if (!in_tag) {
            if (c == '<') {
                in_tag = true;
                try out.append(allocator, c);
                i += 1;
                continue;
            }
            if (c == '{' and i + 1 < html.len and html[i+1] != '#' and html[i+1] != '/' and html[i+1] != ':') {
                var depth: usize = 1;
                var j = i + 1;
                while (j < html.len and depth > 0) {
                    if (html[j] == '{') depth += 1
                    else if (html[j] == '}') depth -= 1;
                    j += 1;
                }
                if (depth == 0) {
                    var expr: []const u8 = try allocator.dupe(u8, html[i + 1 .. j - 1]);
                    
                    for (signals) |sig| {
                        const new_expr = try replaceWord(allocator, expr, sig, ".value");
                        allocator.free(expr);
                        expr = new_expr;
                    }

                    const id = try std.fmt.allocPrint(allocator, "erm-bind-{d}", .{j});
                    try out.appendSlice(allocator, "<span id=\"");
                    try out.appendSlice(allocator, id);
                    try out.appendSlice(allocator, "\"></span>");
                    
                    const binding = try std.fmt.allocPrint(allocator, "window.__erm_bindings.push({{ id: \"{s}\", get: () => ({s}) }});", .{id, expr});
                    try bindings.append(allocator, binding);
                    allocator.free(expr);
                    i = j;
                    continue;
                }
            }
        } else {
            if (c == '>') {
                in_tag = false;
                try out.append(allocator, c);
                i += 1;
                continue;
            }
            if (i > 0 and std.ascii.isWhitespace(html[i-1]) and std.mem.startsWith(u8, html[i..], "on")) {
                var k = i + 2;
                while (k < html.len and std.ascii.isAlphabetic(html[k])) k += 1;
                if (k < html.len and html[k] == '=') {
                    const attr_name = html[i..k];
                    if (k + 1 < html.len and html[k+1] == '{') {
                        var depth: usize = 1;
                        var j = k + 2;
                        while (j < html.len and depth > 0) {
                            if (html[j] == '{') depth += 1
                            else if (html[j] == '}') depth -= 1;
                            j += 1;
                        }
                        if (depth == 0) {
                            var expr: []const u8 = try allocator.dupe(u8, html[k + 2 .. j - 1]);

                            for (signals) |sig| {
                                const new_expr = try replaceWord(allocator, expr, sig, ".value");
                                allocator.free(expr);
                                expr = new_expr;
                            }

                            const event_type_raw = attr_name[2..];
                            const event_type = try allocator.dupe(u8, event_type_raw);
                            for (event_type) |*char| char.* = std.ascii.toLower(char.*);
                            
                            const id = try std.fmt.allocPrint(allocator, "erm-evt-{d}", .{j});
                            try out.appendSlice(allocator, "id=\"");
                            try out.appendSlice(allocator, id);
                            try out.appendSlice(allocator, "\" ");
                            
                            const event = try std.fmt.allocPrint(allocator, "window.__erm_events.push({{ id: \"{s}\", event: \"{s}\", handler: (event) => {{ ({s})(event); if (typeof window.__erm_update === 'function') window.__erm_update(); }} }});", .{id, event_type, expr});
                            try events.append(allocator, event);
                            allocator.free(expr);
                            allocator.free(event_type);
                            i = j;
                            continue;
                        }
                    }
                }
            }
        }
        try out.append(allocator, c);
        i += 1;
    }
    return out.toOwnedSlice(allocator);
}

fn replaceWord(allocator: std.mem.Allocator, input: []const u8, word: []const u8, suffix: []const u8) ![]const u8 {
    if (word.len <= 1) return try allocator.dupe(u8, input); // Extra safety
    var res: std.ArrayList(u8) = .empty;
    var i: usize = 0;
    while (i < input.len) {
        if (std.mem.startsWith(u8, input[i..], word)) {
            const end = i + word.len;
            const before_ok = if (i == 0) true else !std.ascii.isAlphanumeric(input[i - 1]) and input[i-1] != '_' and input[i-1] != '$';
            const after_ok = if (end == input.len) true else !std.ascii.isAlphanumeric(input[end]) and input[end] != '_' and input[end] != '$';
            
            if (before_ok and after_ok) {
                var is_decl = false;
                const keywords = [_][]const u8{ "let", "const", "var" };
                for (keywords) |kw| {
                    if (i >= kw.len + 1) {
                        const start = i - kw.len - 1;
                        if (std.mem.eql(u8, input[start..i-1], kw) and std.ascii.isWhitespace(input[i-1])) {
                             const pre_kw_ok = if (start == 0) true else !std.ascii.isAlphanumeric(input[start-1]);
                             if (pre_kw_ok) {
                                 is_decl = true;
                                 break;
                             }
                        }
                    }
                }

                if (is_decl) {
                    try res.appendSlice(allocator, word);
                } else if (std.mem.startsWith(u8, input[end..], suffix)) {
                    try res.appendSlice(allocator, word);
                } else {
                    try res.appendSlice(allocator, word);
                    try res.appendSlice(allocator, suffix);
                }
                i = end;
                continue;
            }
        }
        try res.append(allocator, input[i]);
        i += 1;
    }
    return res.toOwnedSlice(allocator);
}

pub fn processComponentTree(allocator: std.mem.Allocator, _ : []const u8, content: []const u8, _ : *std.StringHashMap(bool)) !ProcessResult {
    var scripts: std.ArrayList([]const u8) = .empty;
    var styles: std.ArrayList([]const u8) = .empty;
    var signal_vars_list: std.ArrayList([]const u8) = .empty;
    defer {
        for (signal_vars_list.items) |s| allocator.free(s);
        signal_vars_list.deinit(allocator);
    }

    var h = std.hash.Fnv1a_32.init();
    h.update(content);
    const hash_val = h.final();
    var scope_id_buf: [32]u8 = undefined;
    const scope_id = try std.fmt.bufPrint(&scope_id_buf, "data-e-{x}", .{hash_val});

    var html_buf: std.ArrayList(u8) = .empty;
    var i: usize = 0;
    while (i < content.len) {
        if (std.mem.startsWith(u8, content[i..], "<script")) {
            const end = std.mem.indexOf(u8, content[i..], "</script>") orelse break;
            const script_tag = content[i .. i + end + 9];
            const content_start = (std.mem.indexOfScalar(u8, script_tag, '>') orelse 0) + 1;
            const script_content = std.mem.trim(u8, script_tag[content_start .. script_tag.len - 9], " \t\n\r");
            
            var sj: usize = 0;
            while (sj < script_content.len) {
                const signal_call = "signal(";
                const signal_idx = std.mem.indexOf(u8, script_content[sj..], signal_call);
                if (signal_idx) |idx| {
                    const call_pos = sj + idx;
                    // Find '=' before signal(
                    var eq_pos: ?usize = null;
                    var k = call_pos;
                    while (k > sj) {
                        k -= 1;
                        if (script_content[k] == '=') {
                            eq_pos = k;
                            break;
                        }
                        if (script_content[k] == ';' or script_content[k] == '{' or script_content[k] == '}') break;
                    }

                    if (eq_pos) |ep| {
                        // Find name before '='
                        var name_end: ?usize = null;
                        var m = ep;
                        while (m > sj) {
                            m -= 1;
                            if (std.ascii.isWhitespace(script_content[m])) continue;
                            if (std.ascii.isAlphanumeric(script_content[m]) or script_content[m] == '_' or script_content[m] == '$') {
                                name_end = m + 1;
                                break;
                            }
                            break;
                        }

                        if (name_end) |ne| {
                            var m_start = ne;
                            while (m_start > sj) {
                                m_start -= 1;
                                if (!(std.ascii.isAlphanumeric(script_content[m_start]) or script_content[m_start] == '_' or script_content[m_start] == '$')) {
                                    m_start += 1;
                                    break;
                                }
                            }
                            const name = script_content[m_start..ne];
                            if (name.len > 1) {
                                var already = false;
                                for (signal_vars_list.items) |existing| {
                                    if (std.mem.eql(u8, existing, name)) { already = true; break; }
                                }
                                if (!already) try signal_vars_list.append(allocator, try allocator.dupe(u8, name));
                            }
                        }
                    }
                    sj = call_pos + signal_call.len;
                } else break;
            }

            try scripts.append(allocator, try allocator.dupe(u8, script_content));
            i += end + 9;
        } else if (std.mem.startsWith(u8, content[i..], "<style")) {
            const end = std.mem.indexOf(u8, content[i..], "</style>") orelse break;
            const style_tag = content[i .. i + end + 8];
            const content_start = (std.mem.indexOfScalar(u8, style_tag, '>') orelse 0) + 1;
            const style_content = style_tag[content_start .. style_tag.len - 8];
            try styles.append(allocator, try scopeCSS(allocator, std.mem.trim(u8, style_content, " \t\n\r"), scope_id));
            i += end + 8;
        } else {
            try html_buf.append(allocator, content[i]);
            i += 1;
        }
    }

    for (scripts.items, 0..) |s, si| {
        var transformed: []const u8 = try allocator.dupe(u8, s);
        for (signal_vars_list.items) |sig| {
            const new_transformed = try replaceWord(allocator, transformed, sig, ".value");
            allocator.free(transformed);
            transformed = new_transformed;
        }
        scripts.items[si] = transformed;
    }

    var bindings: std.ArrayList([]const u8) = .empty;
    var events: std.ArrayList([]const u8) = .empty;
    const reactive_html = try parseReactivity(allocator, html_buf.items, &bindings, &events, signal_vars_list.items);
    html_buf.deinit(allocator);

    const scoped_html = try scopeHTML(allocator, reactive_html, scope_id);
    allocator.free(reactive_html);

    for (bindings.items) |b| try scripts.append(allocator, b);
    for (events.items) |e| try scripts.append(allocator, e);
    bindings.deinit(allocator);
    events.deinit(allocator);

    return .{
        .html = scoped_html,
        .scripts = scripts,
        .styles = styles,
        .signal_vars = std.StringHashMap([]const u8).init(allocator),
    };
}

pub fn processErmComponent(allocator: std.mem.Allocator, base_dir: []const u8, content: []const u8) ![]const u8 {
    var visited = std.StringHashMap(bool).init(allocator);
    defer visited.deinit();
    
    const result = try processComponentTree(allocator, base_dir, content, &visited);
    
    var final: std.ArrayList(u8) = .empty;

    // HMR Client Script (at the top to catch all listeners/intervals from the start)
    const hmr_script = 
        \\<script>
        \\(function() {
        \\  if (window.__hmr_initialized) return;
        \\  window.__hmr_initialized = true;
        \\  console.log("[HMR] Initialized");
        \\
        \\  window.__hmr_hooks = window.__hmr_hooks || { dispose: [], accept: [] };
        \\  window.hmr = {
        \\    data: window.__hmr_data || {},
        \\    accept: (cb) => window.__hmr_hooks.accept.push(cb),
        \\    dispose: (cb) => window.__hmr_hooks.dispose.push(cb),
        \\    invalidate: () => location.reload()
        \\  };
        \\  window.__hmr_data = window.hmr.data;
        \\
        \\  window.__hmr_intervals = window.__hmr_intervals || [];
        \\  const originalSetInterval = window.setInterval;
        \\  window.setInterval = function(fn, t) {
        \\    let id = originalSetInterval(fn, t);
        \\    window.__hmr_intervals.push(id);
        \\    return id;
        \\  };
        \\
        \\  window.__hmr_listeners = window.__hmr_listeners || [];
        \\  const originalDocAddEventListener = document.addEventListener;
        \\  document.addEventListener = function(type, listener, options) {
        \\    window.__hmr_listeners.push({ target: document, type, listener, options });
        \\    return originalDocAddEventListener.call(document, type, listener, options);
        \\  };
        \\
        \\  const originalWinAddEventListener = window.addEventListener;
        \\  window.addEventListener = function(type, listener, options) {
        \\    window.__hmr_listeners.push({ target: window, type, listener, options });
        \\    return originalWinAddEventListener.call(window, type, listener, options);
        \\  };
        \\
        \\  const es = new EventSource("/__hmr");
        \\  es.onmessage = (e) => {
        \\    const data = JSON.parse(e.data);
        \\    if (data.type === 'reload') {
        \\      location.reload();
        \\    } else if (data.type === 'update') {
        \\      console.log("[HMR] Update received");
        \\
        \\      window.__hmr_hooks.dispose.forEach(cb => { try { cb(window.hmr.data); } catch(err) {} });
        \\      window.__hmr_hooks.dispose = [];
        \\      window.__hmr_hooks.accept = [];
        \\
        \\      window.__hmr_intervals.forEach(clearInterval);
        \\      window.__hmr_intervals = [];
        \\
        \\      window.__hmr_listeners.forEach(({ target, type, listener, options }) => {
        \\        target.removeEventListener(type, listener, options);
        \\      });
        \\      window.__hmr_listeners = [];
        \\
        \\      fetch(location.href)
        \\        .then(r => r.text())
        \\        .then(html => {
        \\          const parser = new DOMParser();
        \\          const doc = parser.parseFromString(html, 'text/html');
        \\          document.title = doc.title;
        \\
        \\          let oldStyles = document.querySelectorAll('style, link[rel="stylesheet"]');
        \\          oldStyles.forEach(s => s.remove());
        \\          let newStyles = doc.querySelectorAll('style, link[rel="stylesheet"]');
        \\          newStyles.forEach(s => document.head.appendChild(s.cloneNode(true)));
        \\
        \\          document.body.innerHTML = doc.body.innerHTML;
        \\          const scripts = document.body.querySelectorAll('script');
        \\          scripts.forEach(s => {
        \\            if (s.textContent.includes("__hmr_initialized")) return;
        \\            const newScript = document.createElement('script');
        \\            newScript.text = s.innerHTML;
        \\            if(s.src) {
        \\               let sUrl = new URL(s.src, location.href);
        \\               sUrl.searchParams.set('t', new Date().getTime());
        \\               newScript.src = sUrl.href;
        \\            }
        \\            s.replaceWith(newScript);
        \\          });
        \\          document.dispatchEvent(new Event('DOMContentLoaded'));
        \\          window.dispatchEvent(new Event('load'));
        \\        });
        \\    }
        \\  };
        \\})();
        \\</script>
    ;
    try final.appendSlice(allocator, hmr_script);
    try final.appendSlice(allocator, result.html);
    
    if (result.styles.items.len > 0) {
        try final.appendSlice(allocator, "\n<style>\n");
        for (result.styles.items) |s| {
            try final.appendSlice(allocator, s);
            try final.append(allocator, '\n');
        }
        try final.appendSlice(allocator, "</style>\n");
    }
    
    const runtime = 
        \\(() => {
        \\  let activeEffect = null;
        \\  window.signal = function(val) {
        \\    const subscribers = new Set();
        \\    const container = { 
        \\      _val: val,
        \\      toString() { return this._val; },
        \\      valueOf() { return this._val; },
        \\      [Symbol.toPrimitive]() { return this._val; }
        \\    };
        \\    return new Proxy(container, {
        \\      get(target, prop) {
        \\        if (prop === 'value') {
        \\          if (activeEffect) subscribers.add(activeEffect);
        \\          return target._val;
        \\        }
        \\        return target[prop];
        \\      },
        \\      set(target, prop, newVal) {
        \\        if (prop === 'value') {
        \\          target._val = newVal;
        \\          subscribers.forEach(fn => fn());
        \\          if (window.__erm_update) window.__erm_update();
        \\          return true;
        \\        }
        \\        target[prop] = newVal;
        \\        return true;
        \\      }
        \\    });
        \\  };
        \\  window.__erm_bindings = [];
        \\  window.__erm_events = [];
        \\  let _updateQueued = false;
        \\  window.__erm_update = function() {
        \\    if (_updateQueued) return;
        \\    _updateQueued = true;
        \\    requestAnimationFrame(() => {
        \\      window.__erm_bindings.forEach(b => {
        \\        try {
        \\          let val = b.get();
        \\          if (b.last !== val) {
        \\            b.last = val;
        \\            let el = document.getElementById(b.id);
        \\            if (el) el.innerText = val;
        \\          }
        \\        } catch(e) {}
        \\      });
        \\      _updateQueued = false;
        \\    });
        \\  };
        \\  document.addEventListener('DOMContentLoaded', () => {
        \\    window.__erm_events.forEach(ev => {
        \\      let el = document.getElementById(ev.id);
        \\      if (el) el.addEventListener(ev.event, ev.handler);
        \\    });
        \\    window.__erm_update();
        \\  });
    ;

    try final.appendSlice(allocator, "<script>\n");
    try final.appendSlice(allocator, runtime);
    try final.appendSlice(allocator, "\n");
    for (result.scripts.items) |s| {
        try final.appendSlice(allocator, s);
        try final.append(allocator, '\n');
    }
    try final.appendSlice(allocator, "})();\n</script>\n");
    
    return final.toOwnedSlice(allocator);
}
