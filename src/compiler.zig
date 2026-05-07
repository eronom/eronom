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
    signal_vars: std.ArrayList([]const u8),
};

fn parseReactivity(allocator: std.mem.Allocator, html: []const u8, bindings: *std.ArrayList([]const u8), events: *std.ArrayList([]const u8), signals: []const []const u8) ![]const u8 {
    var out: std.ArrayList(u8) = .empty;
    var i: usize = 0;
    var in_tag = false;

    var block_depth: usize = 0;
    while (i < html.len) {
        const c = html[i];

        // Track control flow block depth
        if (c == '{' and i + 1 < html.len) {
            if (html[i+1] == '#') {
                block_depth += 1;
            } else if (html[i+1] == '/') {
                if (block_depth > 0) block_depth -= 1;
            }
        }

        if (!in_tag) {
            if (c == '<') {
                in_tag = true;
                try out.append(allocator, c);
                i += 1;
                continue;
            }
            // Only process as reactive binding if NOT inside a control flow block
            if (block_depth == 0 and c == '{' and i + 1 < html.len and html[i + 1] != '#' and html[i + 1] != '/' and html[i + 1] != ':') {
                var depth: usize = 1;
                var j = i + 1;
                while (j < html.len and depth > 0) {
                    if (html[j] == '{') depth += 1 else if (html[j] == '}') depth -= 1;
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

                    const binding = try std.fmt.allocPrint(allocator, "window.__erm_bindings.push({{ id: \"{s}\", get: () => ({s}) }});", .{ id, expr });
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
            if (i > 0 and std.ascii.isWhitespace(html[i - 1]) and std.mem.startsWith(u8, html[i..], "on")) {
                var k = i + 2;
                while (k < html.len and std.ascii.isAlphabetic(html[k])) k += 1;
                if (k < html.len and html[k] == '=') {
                    const attr_name = html[i..k];
                    if (k + 1 < html.len and html[k + 1] == '{') {
                        var depth: usize = 1;
                        var j = k + 2;
                        while (j < html.len and depth > 0) {
                            if (html[j] == '{') depth += 1 else if (html[j] == '}') depth -= 1;
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

                            const event = try std.fmt.allocPrint(allocator, "window.__erm_events.push({{ id: \"{s}\", event: \"{s}\", handler: (event) => {{ ({s})(event); if (typeof window.__erm_update === 'function') window.__erm_update(); }} }});", .{ id, event_type, expr });
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
    var in_string: ?u8 = null;
    while (i < input.len) {
        if (in_string) |quote| {
            if (input[i] == quote and (i == 0 or input[i - 1] != '\\')) {
                in_string = null;
            }
            try res.append(allocator, input[i]);
            i += 1;
            continue;
        }

        if (input[i] == '"' or input[i] == '\'') {
            in_string = input[i];
            try res.append(allocator, input[i]);
            i += 1;
            continue;
        }

        if (std.mem.startsWith(u8, input[i..], word)) {
            const end = i + word.len;
            const before_ok = if (i == 0) true else !std.ascii.isAlphanumeric(input[i - 1]) and input[i - 1] != '_' and input[i - 1] != '$';
            const after_ok = if (end == input.len) true else !std.ascii.isAlphanumeric(input[end]) and input[end] != '_' and input[end] != '$';

            if (before_ok and after_ok) {
                var is_decl = false;
                const keywords = [_][]const u8{ "let", "const", "var" };
                for (keywords) |kw| {
                    if (i >= kw.len + 1) {
                        const start = i - kw.len - 1;
                        if (std.mem.eql(u8, input[start .. i - 1], kw) and std.ascii.isWhitespace(input[i - 1])) {
                            const pre_kw_ok = if (start == 0) true else !std.ascii.isAlphanumeric(input[start - 1]);
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

fn injectSignalName(allocator: std.mem.Allocator, input: []const u8, name: []const u8) ![]const u8 {
    var res: std.ArrayList(u8) = .empty;
    defer res.deinit(allocator);
    var i: usize = 0;
    while (i < input.len) {
        if (std.mem.startsWith(u8, input[i..], name)) {
            const end = i + name.len;
            var k = end;
            while (k < input.len and (std.ascii.isWhitespace(input[k]) or input[k] == ':')) k += 1;
            if (k < input.len and input[k] == '=') {
                k += 1;
                while (k < input.len and std.ascii.isWhitespace(input[k])) k += 1;
                if (std.mem.startsWith(u8, input[k..], "signal(")) {
                    var depth: usize = 1;
                    var j = k + 7;
                    while (j < input.len and depth > 0) {
                        if (input[j] == '(') depth += 1 else if (input[j] == ')') depth -= 1;
                        j += 1;
                    }
                    if (depth == 0) {
                        try res.appendSlice(allocator, input[i .. j - 1]);
                        try res.appendSlice(allocator, ", \"");
                        try res.appendSlice(allocator, name);
                        try res.appendSlice(allocator, "\")");

                        const remaining = try injectSignalName(allocator, input[j..], name);
                        defer allocator.free(remaining);
                        try res.appendSlice(allocator, remaining);
                        return res.toOwnedSlice(allocator);
                    }
                }
            }
        }
        try res.append(allocator, input[i]);
        i += 1;
    }
    return res.toOwnedSlice(allocator);
}

pub fn processComponentTree(allocator: std.mem.Allocator, _: []const u8, content: []const u8, _: *std.StringHashMap(bool)) !ProcessResult {
    var scripts: std.ArrayList([]const u8) = .empty;
    var styles: std.ArrayList([]const u8) = .empty;
    var signal_vars_list: std.ArrayList([]const u8) = .empty;

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
                                    if (std.mem.eql(u8, existing, name)) {
                                        already = true;
                                        break;
                                    }
                                }
                                if (!already) {
                                    try signal_vars_list.append(allocator, try allocator.dupe(u8, name));
                                }
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
            const new_injected = try injectSignalName(allocator, transformed, sig);
            allocator.free(transformed);
            transformed = new_injected;
        }
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
        .signal_vars = signal_vars_list,
    };
}

pub fn processErmComponent(allocator: std.mem.Allocator, base_dir: []const u8, content: []const u8) ![]const u8 {
    var visited = std.StringHashMap(bool).init(allocator);
    defer visited.deinit();

    var result = try processComponentTree(allocator, base_dir, content, &visited);

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
        \\  const originalElementAddEventListener = Element.prototype.addEventListener;
        \\  Element.prototype.addEventListener = function(type, listener, options) {
        \\    window.__hmr_listeners.push({ target: this, type, listener, options });
        \\    return originalElementAddEventListener.call(this, type, listener, options);
        \\  };
        \\
        \\  const es = new EventSource("/__hmr");
        \\  es.onmessage = (e) => {
        \\    const data = JSON.parse(e.data);
        \\    if (data.type === 'reload') {
        \\      location.reload();
        \\    } else if (data.type === 'update') {
        \\      const path = data.path || 'unknown';
        \\      console.log("[HMR] Update received for: " + path);
        \\
        \\      if (path.endsWith('.css')) {
        \\          let links = document.querySelectorAll('link[rel="stylesheet"]');
        \\          let found = false;
        \\          links.forEach(link => {
        \\              if (link.href.includes(path)) {
        \\                  link.href = path + '?t=' + new Date().getTime();
        \\                  found = true;
        \\              }
        \\          });
        \\          if (found) return;
        \\      }
        \\
        \\      fetch(location.href)
        \\        .then(r => r.text())
        \\        .then(html => {
        \\          const parser = new DOMParser();
        \\          const doc = parser.parseFromString(html, 'text/html');
        \\          document.title = doc.title;
        \\
        \\          function morph(oldNode, newNode) {
        \\            if (oldNode.nodeType !== newNode.nodeType || oldNode.tagName !== newNode.tagName) {
        \\              oldNode.replaceWith(newNode.cloneNode(true));
        \\              return;
        \\            }
        \\            if (oldNode.nodeType === Node.TEXT_NODE) {
        \\              if (oldNode.textContent !== newNode.textContent) oldNode.textContent = newNode.textContent;
        \\              return;
        \\            }
        \\            const oldAttrs = oldNode.attributes;
        \\            const newAttrs = newNode.attributes;
        \\            if (oldAttrs && newAttrs) {
        \\              for (let i = 0; i < newAttrs.length; i++) {
        \\                const attr = newAttrs[i];
        \\                if (oldNode.getAttribute(attr.name) !== attr.value) oldNode.setAttribute(attr.name, attr.value);
        \\              }
        \\              for (let i = 0; i < oldAttrs.length; i++) {
        \\                const attr = oldAttrs[i];
        \\                if (!newNode.hasAttribute(attr.name)) oldNode.removeAttribute(attr.name);
        \\              }
        \\            }
        \\            const oldChildren = Array.from(oldNode.childNodes);
        \\            const newChildren = Array.from(newNode.childNodes);
        \\            const max = Math.max(oldChildren.length, newChildren.length);
        \\            for (let i = 0; i < max; i++) {
        \\              if (i >= oldChildren.length) {
        \\                oldNode.appendChild(newChildren[i].cloneNode(true));
        \\              } else if (i >= newChildren.length) {
        \\                oldNode.removeChild(oldChildren[i]);
        \\              } else {
        \\                morph(oldChildren[i], newChildren[i]);
        \\              }
        \\            }
        \\          }
        \\
        \\          const newStyles = doc.querySelectorAll('style');
        \\          if (newStyles.length > 0) {
        \\              let styleContainer = document.getElementById('__erm_styles');
        \\              if (!styleContainer) {
        \\                  styleContainer = document.createElement('div');
        \\                  styleContainer.id = '__erm_styles';
        \\                  document.head.appendChild(styleContainer);
        \\              }
        \\              styleContainer.innerHTML = '';
        \\              newStyles.forEach(s => styleContainer.appendChild(s.cloneNode(true)));
        \\          }
        \\
        \\          // Call dispose handlers before DOM replacement
        \\          window.__hmr_hooks.dispose.forEach(cb => { try { cb(window.hmr.data); } catch(err) {} });
        \\          window.__hmr_hooks.dispose = [];
        \\          window.__hmr_hooks.accept = [];
        \\          window.__hmr_intervals.forEach(clearInterval);
        \\          window.__hmr_intervals = [];
        \\          window.__hmr_listeners.forEach(({ target, type, listener, options }) => {
        \\            target.removeEventListener(type, listener, options);
        \\            if (target.__erm_listener_added) delete target.__erm_listener_added;
        \\          });
        \\          window.__hmr_listeners = [];
        \\
        \\          morph(document.body, doc.body);
        \\
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
        \\          if (window.__erm_update) window.__erm_update();
        \\        });
        \\    }
        \\  };
        \\})();
        \\</script>
    ;
    try final.appendSlice(allocator, hmr_script);

    var ev = eval.ErmEval.init(allocator);
    defer ev.deinit();

    var script_all: std.ArrayList(u8) = .empty;
    defer script_all.deinit(allocator);
    for (result.scripts.items) |s| {
        try script_all.appendSlice(allocator, s);
        try script_all.append(allocator, '\n');
    }
    try ev.parseScriptVars(script_all.items);

    var res_html = try allocator.dupe(u8, result.html);
    defer allocator.free(res_html);

    // Process {#for}
    while (true) {
        const start_idx = std.mem.indexOf(u8, res_html, "{#for ") orelse break;
        const end_for_idx = std.mem.indexOf(u8, res_html[start_idx..], "{/for}") orelse break;
        const full_end_idx = start_idx + end_for_idx + 6;

        const header_start = start_idx + 6;
        const header_end = std.mem.indexOfScalar(u8, res_html[header_start..], '}') orelse break;
        const full_header_end = header_start + header_end;

        const header = res_html[header_start..full_header_end];
        const body = res_html[full_header_end + 1 .. start_idx + end_for_idx];

        // Parse "item, i in items"
        const in_idx = std.mem.indexOf(u8, header, " in ") orelse break;
        const vars_part = std.mem.trim(u8, header[0..in_idx], " ");
        const collection_expr_raw = std.mem.trim(u8, header[in_idx + 4 ..], " ");

        // Transform signal vars in collection_expr
        var collection_expr: []const u8 = try allocator.dupe(u8, collection_expr_raw);
        defer allocator.free(collection_expr);
        for (result.signal_vars.items) |sig| {
            const new_ce = try replaceWord(allocator, collection_expr, sig, ".value");
            allocator.free(collection_expr);
            collection_expr = new_ce;
        }

        var item_name: []const u8 = "";
        var index_name: []const u8 = "";
        if (std.mem.indexOfScalar(u8, vars_part, ',')) |comma_idx| {
            item_name = std.mem.trim(u8, vars_part[0..comma_idx], " ");
            index_name = std.mem.trim(u8, vars_part[comma_idx + 1 ..], " ");
        } else {
            item_name = vars_part;
        }

        const anchor_id = try std.fmt.allocPrint(allocator, "erm-for-{d}", .{std.time.nanoTimestamp()});
        defer allocator.free(anchor_id);

        // SSR For Loop
        var ssr_html: std.ArrayList(u8) = .empty;
        defer ssr_html.deinit(allocator);

        const collection_val = ev.eval(collection_expr) catch eval.Value{ .null = {} };
        if (collection_val == .list) {
            for (collection_val.list.items, 0..) |item, idx| {
                var sub_ev = try ev.clone();
                defer sub_ev.deinit();
                try sub_ev.set_owned(item_name, try item.clone(allocator));
                if (index_name.len > 0) {
                    try sub_ev.set_owned(index_name, .{ .number = @floatFromInt(idx) });
                }

                // Replace {expr} in body
                var bit: usize = 0;
                while (bit < body.len) {
                    if (body[bit] == '{' and bit + 1 < body.len and body[bit + 1] != '#' and body[bit + 1] != '/' and body[bit + 1] != ':') {
                        var depth: usize = 1;
                        var j = bit + 1;
                        while (j < body.len and depth > 0) {
                            if (body[j] == '{') depth += 1 else if (body[j] == '}') depth -= 1;
                            j += 1;
                        }
                        if (depth == 0) {
                            var sub_expr: []const u8 = try allocator.dupe(u8, body[bit + 1 .. j - 1]);
                            defer allocator.free(sub_expr);
                            for (result.signal_vars.items) |sig| {
                                const new_se = try replaceWord(allocator, sub_expr, sig, ".value");
                                allocator.free(sub_expr);
                                sub_expr = new_se;
                            }

                            const val_iter = sub_ev.eval(sub_expr) catch eval.Value{ .null = {} };
                            var val_buf: [1024]u8 = undefined;
                            const val_str = try std.fmt.bufPrint(&val_buf, "{f}", .{val_iter});
                            try ssr_html.appendSlice(allocator, val_str);
                            bit = j;
                            continue;
                        }
                    }
                    try ssr_html.append(allocator, body[bit]);
                    bit += 1;
                }
            }
        }

        const body_b64 = try allocator.alloc(u8, std.base64.standard.Encoder.calcSize(body.len));
        _ = std.base64.standard.Encoder.encode(body_b64, body);
        defer allocator.free(body_b64);

        const js_params = if (index_name.len > 0) try std.fmt.allocPrint(allocator, "{s}, {s}", .{ item_name, index_name }) else try allocator.dupe(u8, item_name);
        defer allocator.free(js_params);

        const logic = try std.fmt.allocPrint(allocator,
            \\  window.__erm_bindings.push({{
            \\    update: () => {{
            \\      let __erm_anchor = document.getElementById("{s}");
            \\      if (__erm_anchor) {{
            \\        let __erm_items = [];
            \\        try {{ __erm_items = ({s}); }} catch(e) {{}}
            \\        if (!Array.isArray(__erm_items)) __erm_items = [];
            \\        let __erm_itemsJson = JSON.stringify(__erm_items);
            \\        if (__erm_anchor.__erm_last_items !== __erm_itemsJson) {{
            \\          __erm_anchor.__erm_last_items = __erm_itemsJson;
            \\          let __erm_template = atob("{s}");
            \\          let __erm_html = "";
            \\          __erm_items.forEach(({s}) => {{
            \\            let __erm_iter_html = __erm_template.replace(/\{{([^{{}}#/:][^{{}}]*)\}}/g, (m, expr) => {{
            \\              try {{ 
            \\                let val = eval(expr); 
            \\                return val === undefined ? "" : val;
            \\              }} catch(e) {{ return ""; }}
            \\            }});
            \\            __erm_html += __erm_iter_html;
            \\          }});
            \\          __erm_anchor.innerHTML = __erm_html;
            \\        }}
            \\      }}
            \\    }}
            \\  }});
        , .{ anchor_id, collection_expr, body_b64, js_params });
        try result.scripts.append(allocator, logic);

        const anchor_html = try std.fmt.allocPrint(allocator, "<span id=\"{s}\" style=\"display:contents;\">{s}</span>", .{ anchor_id, ssr_html.items });
        defer allocator.free(anchor_html);

        const new_res = try std.mem.replaceOwned(u8, allocator, res_html, res_html[start_idx..full_end_idx], anchor_html);
        allocator.free(res_html);
        res_html = new_res;
    }

    // Process {#if}
    while (true) {
        const start_idx = std.mem.indexOf(u8, res_html, "{#if ") orelse break;
        var end_if_idx: ?usize = null;
        var depth: usize = 1;
        var j = start_idx + 5;
        while (j < res_html.len) {
            if (std.mem.startsWith(u8, res_html[j..], "{#if ")) {
                depth += 1;
                j += 5;
            } else if (std.mem.startsWith(u8, res_html[j..], "{/if}")) {
                depth -= 1;
                if (depth == 0) {
                    end_if_idx = j;
                    break;
                }
                j += 5;
            } else {
                j += 1;
            }
        }

        if (end_if_idx == null) break;
        const full_block = res_html[start_idx .. end_if_idx.? + 5];

        const anchor_id = try std.fmt.allocPrint(allocator, "erm-if-{d}", .{std.time.nanoTimestamp()});
        defer allocator.free(anchor_id);

        var branches: std.ArrayList(u8) = .empty;
        defer branches.deinit(allocator);

        var ssr_html_res: ?[]const u8 = null;
        var ssr_found = false;

        var rem = full_block;
        while (rem.len > 0) {
            var block_type: enum { if_stmt, elseif_stmt, else_stmt } = .if_stmt;
            var cond_expr_raw: []const u8 = "true";
            var body_start: usize = 0;

            if (std.mem.startsWith(u8, rem, "{#if ")) {
                block_type = .if_stmt;
                const end_brace = std.mem.indexOfScalar(u8, rem, '}') orelse break;
                cond_expr_raw = std.mem.trim(u8, rem[5..end_brace], " ");
                body_start = end_brace + 1;
            } else if (std.mem.startsWith(u8, rem, "{:else if ")) {
                block_type = .elseif_stmt;
                const end_brace = std.mem.indexOfScalar(u8, rem, '}') orelse break;
                cond_expr_raw = std.mem.trim(u8, rem[10..end_brace], " ");
                body_start = end_brace + 1;
            } else if (std.mem.startsWith(u8, rem, "{:else}")) {
                block_type = .else_stmt;
                cond_expr_raw = "true";
                body_start = 7;
            } else break;

            var cond_expr: []const u8 = try allocator.dupe(u8, cond_expr_raw);
            defer allocator.free(cond_expr);
            for (result.signal_vars.items) |sig| {
                const new_ce = try replaceWord(allocator, cond_expr, sig, ".value");
                allocator.free(cond_expr);
                cond_expr = new_ce;
            }

            const next_else = std.mem.indexOf(u8, rem[body_start..], "{:else") orelse std.mem.indexOf(u8, rem[body_start..], "{/if}") orelse break;
            const body = rem[body_start .. body_start + next_else];

            const body_b64 = try allocator.alloc(u8, std.base64.standard.Encoder.calcSize(body.len));
            _ = std.base64.standard.Encoder.encode(body_b64, body);
            defer allocator.free(body_b64);

            if (!ssr_found) {
                const cond_val = if (block_type == .else_stmt) true else (ev.evalBool(cond_expr) catch false);
                if (cond_val) {
                    ssr_html_res = try allocator.dupe(u8, body);
                    ssr_found = true;
                }
            }

            try branches.appendSlice(allocator, if (block_type == .if_stmt) "if (" else " else if (");
            try branches.appendSlice(allocator, cond_expr);
            try branches.appendSlice(allocator, ") { __erm_new = atob(\"");
            try branches.appendSlice(allocator, body_b64);
            try branches.appendSlice(allocator, "\"); }");

            rem = rem[body_start + next_else ..];
            if (std.mem.startsWith(u8, rem, "{/if}")) break;
        }

        const logic = try std.fmt.allocPrint(allocator,
            \\  window.__erm_bindings.push({{
            \\    update: () => {{
            \\      let __erm_anchor = document.getElementById("{s}");
            \\      if (__erm_anchor) {{
            \\        let __erm_new = "";
            \\        {s}
            \\        if (__erm_anchor.__erm_last !== __erm_new) {{
            \\          __erm_anchor.__erm_last = __erm_new;
            \\          __erm_anchor.innerHTML = __erm_new;
            \\        }}
            \\      }}
            \\    }}
            \\  }});
        , .{ anchor_id, branches.items });
        try result.scripts.append(allocator, logic);

        const anchor_html = try std.fmt.allocPrint(allocator, "<span id=\"{s}\" style=\"display:contents;\">{s}</span>", .{ anchor_id, ssr_html_res orelse "" });
        defer allocator.free(anchor_html);
        if (ssr_html_res) |s| allocator.free(s);

        const new_res = try std.mem.replaceOwned(u8, allocator, res_html, full_block, anchor_html);
        allocator.free(res_html);
        res_html = new_res;
    }

    try final.appendSlice(allocator, res_html);

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
        \\  window.__hmr_data = window.__hmr_data || { signals: {} };
        \\  if (!window.__hmr_data.signals) window.__hmr_data.signals = {};
        \\  let activeEffect = null;
        \\  window.signal = function(val, name) {
        \\    if (name && window.__hmr_data.signals[name] !== undefined) {
        \\      val = window.__hmr_data.signals[name];
        \\    }
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
        \\          if (name) window.__hmr_data.signals[name] = newVal;
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
        \\          if (typeof b.update === 'function') {
        \\            b.update();
        \\          } else {
        \\            let val = b.get();
        \\            if (b.last !== val) {
        \\              b.last = val;
        \\              let el = document.getElementById(b.id);
        \\              if (el) el.innerText = val === undefined ? '' : val;
        \\            }
        \\          }
        \\        } catch(e) {}
        \\      });
        \\      if (typeof _initReactivity === 'function') _initReactivity();
        \\      _updateQueued = false;
        \\    });
        \\  };
        \\  function _initReactivity() {
        \\    window.__erm_events.forEach(ev => {
        \\      let el = document.getElementById(ev.id);
        \\      if (el && !el.__erm_listener_added) {
        \\         el.addEventListener(ev.event, ev.handler);
        \\         el.__erm_listener_added = true;
        \\      }
        \\    });
        \\    window.__erm_update();
        \\  }
        \\  if (document.readyState === 'loading') {
        \\    document.addEventListener('DOMContentLoaded', _initReactivity);
        \\  } else {
        \\    _initReactivity();
        \\  }
        \\  // Also run it after a short delay to catch late-bound elements
        \\  setTimeout(_initReactivity, 10);
    ;

    try final.appendSlice(allocator, "<script>\n");
    try final.appendSlice(allocator, runtime);
    try final.appendSlice(allocator, "\n");
    for (result.scripts.items) |s| {
        try final.appendSlice(allocator, s);
        try final.append(allocator, '\n');
    }
    try final.appendSlice(allocator, "})();\n</script>\n");

    for (result.signal_vars.items) |s| allocator.free(s);
    result.signal_vars.deinit(allocator);

    return final.toOwnedSlice(allocator);
}
