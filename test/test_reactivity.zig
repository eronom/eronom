const std = @import("std");

pub fn main() !void {
    const allocator = std.heap.page_allocator;
    const html = "<div>Count {count}</div><button onClick={()=>{count++}}>Add</button>";
    var bindings: std.ArrayList([]const u8) = .empty;
    defer bindings.deinit(allocator);
    var events: std.ArrayList([]const u8) = .empty;
    defer events.deinit(allocator);
    
    const result = try parseReactivity(allocator, html, &bindings, &events);
    std.debug.print("HTML: {s}\n", .{result});
    for (bindings.items) |b| std.debug.print("Binding: {s}\n", .{b});
    for (events.items) |e| std.debug.print("Event: {s}\n", .{e});
}

fn parseReactivity(allocator: std.mem.Allocator, html: []const u8, bindings: *std.ArrayList([]const u8), events: *std.ArrayList([]const u8)) ![]const u8 {
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
                    const expr = html[i + 1 .. j - 1];
                    const id = try std.fmt.allocPrint(allocator, "erm-bind-{d}", .{j}); // simplified ID
                    try out.appendSlice(allocator, "<span id=\"");
                    try out.appendSlice(allocator, id);
                    try out.appendSlice(allocator, "\"></span>");
                    
                    const binding = try std.fmt.allocPrint(allocator, "window.__erm_bindings.push({{ id: \"{s}\", get: () => ({s}) }});", .{id, expr});
                    try bindings.append(allocator, binding);
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
            // Handle events onEvent={...}
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
                            const expr = html[k + 2 .. j - 1];
                            const event_type = try allocator.dupe(u8, attr_name[2..]);
                            for (event_type) |*char| char.* = std.ascii.toLower(char.*);
                            
                            const id = try std.fmt.allocPrint(allocator, "erm-evt-{d}", .{j});
                            try out.appendSlice(allocator, "id=\"");
                            try out.appendSlice(allocator, id);
                            try out.appendSlice(allocator, "\" ");
                            
                            const event = try std.fmt.allocPrint(allocator, "window.__erm_events.push({{ id: \"{s}\", event: \"{s}\", handler: (event) => {{ {s}; if (typeof window.__erm_update === 'function') window.__erm_update(); }} }});", .{id, event_type, expr});
                            try events.append(allocator, event);
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
