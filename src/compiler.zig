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
                // Skip if it's an animation keyframe or global tag
                if (std.mem.containsAtLeast(u8, trimmed, 1, "%") or 
                    std.mem.eql(u8, trimmed, "to") or 
                    std.mem.eql(u8, trimmed, "from") or 
                    std.mem.startsWith(u8, trimmed, "body") or 
                    std.mem.startsWith(u8, trimmed, "html")) 
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
    // Append remaining
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
            
            // Skip components (Uppercase) and global tags
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

pub fn processComponentTree(allocator: std.mem.Allocator, _ : []const u8, content: []const u8, _ : *std.StringHashMap(bool)) !ProcessResult {
    var scripts: std.ArrayList([]const u8) = .empty;
    var styles: std.ArrayList([]const u8) = .empty;
    const signal_vars = std.StringHashMap([]const u8).init(allocator);

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
            const script_content = script_tag[content_start .. script_tag.len - 9];
            try scripts.append(allocator, try allocator.dupe(u8, std.mem.trim(u8, script_content, " \t\n\r")));
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

    const scoped_html = try scopeHTML(allocator, html_buf.items, scope_id);
    html_buf.deinit(allocator);

    // Re-implementation of reactivity and component recursive call would go here.
    // For now, this is the basic structure.

    return .{
        .html = scoped_html,
        .scripts = scripts,
        .styles = styles,
        .signal_vars = signal_vars,
    };
}

pub fn processErmComponent(allocator: std.mem.Allocator, base_dir: []const u8, content: []const u8) ![]const u8 {
    var visited = std.StringHashMap(bool).init(allocator);
    defer visited.deinit();
    
    const result = try processComponentTree(allocator, base_dir, content, &visited);
    
    // SSR assembly
    var final: std.ArrayList(u8) = .empty;
    try final.appendSlice(allocator, result.html);
    
    if (result.styles.items.len > 0) {
        try final.appendSlice(allocator, "\n<style>\n");
        for (result.styles.items) |s| {
            try final.appendSlice(allocator, s);
            try final.append(allocator, '\n');
        }
        try final.appendSlice(allocator, "</style>\n");
    }
    
    if (result.scripts.items.len > 0) {
        try final.appendSlice(allocator, "<script>\n(() => {\n");
        for (result.scripts.items) |s| {
            try final.appendSlice(allocator, s);
            try final.append(allocator, '\n');
        }
        try final.appendSlice(allocator, "})();\n</script>\n");
    }
    
    return final.toOwnedSlice(allocator);
}
