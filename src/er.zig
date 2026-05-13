const std = @import("std");

pub const Route = struct {
    method: []const u8,
    path: []const u8,
    handler_lines: [][]const u8,
};

pub const Variable = struct {
    value: []const u8,
    is_mutable: bool = true,
    decl_line: usize = 0,
    decl_path: []const u8 = "",
};

pub fn evaluateFile(allocator: std.mem.Allocator, io: std.Io, path: []const u8, variables: *std.StringHashMap(Variable), allocated_keys: *std.ArrayList([]const u8), routes: *std.ArrayList(Route)) !void {
    const content = std.Io.Dir.cwd().readFileAlloc(io, path, allocator, @enumFromInt(1024 * 1024)) catch |err| {
        if (err == error.FileNotFound) return;
        return err;
    };
    defer allocator.free(content);

    var line_count: usize = 0;
    var line_it_count = std.mem.splitSequence(u8, content, "\n");
    while (line_it_count.next()) |_| line_count += 1;

    var lines = try allocator.alloc([]const u8, line_count);
    defer allocator.free(lines);

    var line_it = std.mem.splitSequence(u8, content, "\n");
    var line_idx: usize = 0;
    while (line_it.next()) |line| {
        lines[line_idx] = line;
        line_idx += 1;
    }

    var if_was_executed = false;
    try executeStatements(allocator, io, lines, variables, allocated_keys, &if_was_executed, routes, path, 0);
}

pub fn runFile(allocator: std.mem.Allocator, io: std.Io, path: []const u8) !void {
    var variables = std.StringHashMap(Variable).init(allocator);
    var allocated_keys: std.ArrayList([]const u8) = .empty;
    defer {
        var it = variables.valueIterator();
        while (it.next()) |val| {
            allocator.free(val.value);
        }
        for (allocated_keys.items) |key| {
            allocator.free(key);
        }
        allocated_keys.deinit(allocator);
        variables.deinit();
    }

    var routes: std.ArrayList(Route) = .empty;
    defer {
        for (routes.items) |route| {
            allocator.free(route.path);
            allocator.free(route.handler_lines);
        }
        routes.deinit(allocator);
    }

    try evaluateFile(allocator, io, path, &variables, &allocated_keys, &routes);


}

pub fn handleApiRequest(allocator: std.mem.Allocator, io: std.Io, request: *std.http.Server.Request, api_file_path: []const u8, api_prefix: []const u8) !bool {
    const content = std.Io.Dir.cwd().readFileAlloc(io, api_file_path, allocator, @enumFromInt(1024 * 1024)) catch |err| {
        std.debug.print("Failed to read API file {s}: {any}\n", .{api_file_path, err});
        return false;
    };
    defer allocator.free(content);

    var variables = std.StringHashMap(Variable).init(allocator);
    var allocated_keys: std.ArrayList([]const u8) = .empty;
    defer {
        var it = variables.valueIterator();
        while (it.next()) |val| {
            allocator.free(val.*.value);
        }
        for (allocated_keys.items) |key| {
            allocator.free(key);
        }
        allocated_keys.deinit(allocator);
        variables.deinit();
    }

    var line_count: usize = 0;
    var line_it_count = std.mem.splitSequence(u8, content, "\n");
    while (line_it_count.next()) |_| line_count += 1;

    var lines = try allocator.alloc([]const u8, line_count);
    defer allocator.free(lines);

    var line_it = std.mem.splitSequence(u8, content, "\n");
    var line_idx: usize = 0;
    while (line_it.next()) |line| {
        lines[line_idx] = line;
        line_idx += 1;
    }

    var if_was_executed = false;
    var routes: std.ArrayList(Route) = .empty;
    defer {
        for (routes.items) |route| {
            allocator.free(route.path);
            for (route.handler_lines) |line| allocator.free(line);
            allocator.free(route.handler_lines);
        }
        routes.deinit(allocator);
    }
    try executeStatements(allocator, io, lines, &variables, &allocated_keys, &if_was_executed, &routes, api_file_path, 0);

    const target = request.head.target;
    var clean_target = target;
    if (std.mem.indexOfScalar(u8, clean_target, '?')) |idx| clean_target = clean_target[0..idx];
    if (std.mem.indexOfScalar(u8, clean_target, '#')) |idx| clean_target = clean_target[0..idx];

    var sub_path = clean_target;
    if (api_prefix.len > 0 and std.mem.startsWith(u8, clean_target, api_prefix)) {
        sub_path = clean_target[api_prefix.len..];
    }
    if (sub_path.len == 0) sub_path = "/";

    for (routes.items) |route| {
        if (std.mem.eql(u8, route.method, @tagName(request.head.method))) {
            var match = std.mem.eql(u8, route.path, sub_path);
            if (!match) {
                if (std.mem.eql(u8, route.path, "/") and (std.mem.eql(u8, sub_path, "") or std.mem.eql(u8, sub_path, "/"))) {
                    match = true;
                } else if (sub_path.len > 0 and sub_path[0] == '/') {
                    if (std.mem.eql(u8, route.path, sub_path[1..])) match = true;
                } else if (route.path.len > 0 and route.path[0] == '/') {
                    if (std.mem.eql(u8, route.path[1..], sub_path)) match = true;
                } else if (std.mem.endsWith(u8, sub_path, "/") and sub_path.len > 1) {
                    if (std.mem.eql(u8, route.path, sub_path[0..sub_path.len-1])) match = true;
                }
            }

            if (match) {
                for (route.handler_lines) |h_line| {
                    const h_trimmed = std.mem.trim(u8, h_line, " \t\r");
                    if (std.mem.indexOf(u8, h_trimmed, "c.json(")) |json_idx| {
                        const o_p = std.mem.indexOfPos(u8, h_trimmed, json_idx, "(") orelse continue;
                        const c_p = std.mem.lastIndexOf(u8, h_trimmed, ")") orelse continue;
                        const data_expr = h_trimmed[o_p + 1 .. c_p];
                        const data_val = try evaluateExpression(allocator, data_expr, variables);
                        defer allocator.free(data_val);

                        _ = try request.respond(data_val, .{
                            .status = .ok,
                            .extra_headers = &.{
                                .{ .name = "Content-Type", .value = "application/json" },
                                .{ .name = "Access-Control-Allow-Origin", .value = "*" },
                            },
                        });
                        return true;
                    }
                }
            }
        }
    }

    return false;
}


fn printPrettyError(allocator: std.mem.Allocator, io: std.Io, path: []const u8, lines: [][]const u8, line_idx: usize, msg: []const u8, note_path: ?[]const u8, note_line_idx: ?usize, note_msg: ?[]const u8, var_name: []const u8) void {
    const line_num = line_idx + 1;
    const line_content = lines[line_idx];

    std.debug.print("{d} | {s}\n", .{ line_num, line_content });
    
    // Try to find the variable name in the line for better caret positioning
    var caret_pos: usize = 0;
    if (std.mem.indexOf(u8, line_content, var_name)) |pos| {
        caret_pos = pos;
    }
    
    std.debug.print("    ", .{});
    for (0..caret_pos) |_| std.debug.print(" ", .{});
    std.debug.print("^\n", .{});
    
    std.debug.print("error: {s}\n", .{msg});
    std.debug.print("    at {s}:{d}:{d}\n\n", .{ path, line_num, caret_pos + 1 });

    if (note_path != null and note_line_idx != null and note_msg != null) {
        const n_line_num = note_line_idx.? + 1;
        var n_line_content: []const u8 = "(content unavailable)";
        var owned_content = false;
        
        if (std.mem.eql(u8, path, note_path.?)) {
            n_line_content = lines[note_line_idx.?];
        } else {
            // Load from file
            if (std.Io.Dir.cwd().readFileAlloc(io, note_path.?, allocator, @enumFromInt(1024*1024))) |content| {
                var it = std.mem.splitSequence(u8, content, "\n");
                var curr: usize = 0;
                while (it.next()) |l| {
                    if (curr == note_line_idx.?) {
                        n_line_content = allocator.dupe(u8, l) catch l;
                        owned_content = true;
                        break;
                    }
                    curr += 1;
                }
                allocator.free(content);
            } else |_| {}
        }
        defer if (owned_content) allocator.free(n_line_content);

        std.debug.print("{d} | {s}\n", .{ n_line_num, n_line_content });
        
        var n_caret_pos: usize = 0;
        if (std.mem.indexOf(u8, n_line_content, var_name)) |pos| {
            n_caret_pos = pos;
        }
        
        std.debug.print("    ", .{});
        for (0..n_caret_pos) |_| std.debug.print(" ", .{});
        std.debug.print("^\n", .{});
        
        std.debug.print("note: {s}\n", .{note_msg.?});
        std.debug.print("   at {s}:{d}:{d}\n", .{ note_path.?, n_line_num, n_caret_pos + 1 });
    }
}

fn findClosingBrace(lines: [][]const u8, start_idx: usize) usize {
    var depth: usize = 0;
    var i = start_idx;
    while (i < lines.len) : (i += 1) {
        const trimmed = std.mem.trim(u8, lines[i], " \t\r");
        for (trimmed) |char| {
            if (char == '{' or char == '[') depth += 1;
            if (char == '}' or char == ']') {
                if (depth > 0) {
                    depth -= 1;
                    if (depth == 0) return i;
                }
            }
        }
    }
    return lines.len;
}

fn normalizeKey(allocator: std.mem.Allocator, key: []const u8) anyerror![]const u8 {
    if (std.mem.indexOfScalar(u8, key, '[') == null and std.mem.indexOfScalar(u8, key, ']') == null) {
        return try allocator.dupe(u8, key);
    }
    var res: std.ArrayList(u8) = .empty;
    errdefer res.deinit(allocator);
    for (key) |char| {
        if (char == '[') {
            try res.append(allocator, '.');
        } else if (char == ']') {
            // skip
        } else {
            try res.append(allocator, char);
        }
    }
    return try res.toOwnedSlice(allocator);
}

fn evaluateExpression(allocator: std.mem.Allocator, expr: []const u8, variables: std.StringHashMap(Variable)) anyerror![]const u8 {
    const trimmed_orig = std.mem.trim(u8, expr, " \t");
    if (trimmed_orig.len == 0) return try allocator.dupe(u8, "");

    if (std.mem.startsWith(u8, trimmed_orig, "\"") and std.mem.endsWith(u8, trimmed_orig, "\"")) {
        return try allocator.dupe(u8, trimmed_orig[1 .. trimmed_orig.len - 1]);
    }
    
    if (std.mem.startsWith(u8, trimmed_orig, "[") and std.mem.endsWith(u8, trimmed_orig, "]")) {
        return try allocator.dupe(u8, trimmed_orig);
    }
    
    if (std.mem.startsWith(u8, trimmed_orig, "{") and std.mem.endsWith(u8, trimmed_orig, "}") and std.mem.indexOf(u8, trimmed_orig, ":") != null) {
        return try allocator.dupe(u8, trimmed_orig);
    }

    const trimmed = try normalizeKey(allocator, trimmed_orig);
    defer allocator.free(trimmed);

    const low_ops = [_][]const u8{ "+", "-" };
    for (low_ops) |op| {
        if (std.mem.lastIndexOf(u8, trimmed, op)) |idx| {
            const left_raw = std.mem.trim(u8, trimmed[0..idx], " \t");
            const right_raw = std.mem.trim(u8, trimmed[idx + op.len ..], " \t");

            if (left_raw.len == 0 and std.mem.eql(u8, op, "-")) {
                const right_val = try evaluateExpression(allocator, right_raw, variables);
                defer allocator.free(right_val);
                const right_num = std.fmt.parseInt(i64, right_val, 10) catch return try allocator.dupe(u8, trimmed);
                return try std.fmt.allocPrint(allocator, "{d}", .{-right_num});
            }

            if (left_raw.len > 0 and right_raw.len > 0) {
                const left_val = try evaluateExpression(allocator, left_raw, variables);
                defer allocator.free(left_val);
                const right_val = try evaluateExpression(allocator, right_raw, variables);
                defer allocator.free(right_val);

                const left_num = std.fmt.parseInt(i64, left_val, 10) catch null;
                const right_num = std.fmt.parseInt(i64, right_val, 10) catch null;

                if (left_num != null and right_num != null) {
                    if (std.mem.eql(u8, op, "+")) return try std.fmt.allocPrint(allocator, "{d}", .{left_num.? + right_num.?});
                    if (std.mem.eql(u8, op, "-")) return try std.fmt.allocPrint(allocator, "{d}", .{left_num.? - right_num.?});
                }
            }
        }
    }

    const high_ops = [_][]const u8{ "*", "/" };
    for (high_ops) |op| {
        if (std.mem.lastIndexOf(u8, trimmed, op)) |idx| {
            const left_raw = std.mem.trim(u8, trimmed[0..idx], " \t");
            const right_raw = std.mem.trim(u8, trimmed[idx + op.len ..], " \t");

            if (left_raw.len > 0 and right_raw.len > 0) {
                const left_val = try evaluateExpression(allocator, left_raw, variables);
                defer allocator.free(left_val);
                const right_val = try evaluateExpression(allocator, right_raw, variables);
                defer allocator.free(right_val);

                const left_num = std.fmt.parseInt(i64, left_val, 10) catch null;
                const right_num = std.fmt.parseInt(i64, right_val, 10) catch null;

                if (left_num != null and right_num != null) {
                    if (std.mem.eql(u8, op, "*")) return try std.fmt.allocPrint(allocator, "{d}", .{left_num.? * right_num.?});
                    if (std.mem.eql(u8, op, "/")) {
                        const res = if (right_num.? != 0) @divTrunc(left_num.?, right_num.?) else 0;
                        return try std.fmt.allocPrint(allocator, "{d}", .{res});
                    }
                }
            }
        }
    }

    if (variables.get(trimmed)) |v| {
        return try allocator.dupe(u8, v.value);
    }
    return try allocator.dupe(u8, trimmed);
}

fn evaluateCondition(allocator: std.mem.Allocator, condition: []const u8, variables: std.StringHashMap(Variable)) anyerror!bool {
    const ops = [_][]const u8{ "==", "!=", ">=", "<=", ">", "<" };
    var op: []const u8 = "";
    var op_idx: usize = 0;

    for (ops) |o| {
        if (std.mem.indexOf(u8, condition, o)) |idx| {
            op = o;
            op_idx = idx;
            break;
        }
    }

    if (op.len == 0) return false;

    const left_raw = std.mem.trim(u8, condition[0..op_idx], " \t");
    const right_raw = std.mem.trim(u8, condition[op_idx + op.len ..], " \t");

    const left_val = try evaluateExpression(allocator, left_raw, variables);
    defer allocator.free(left_val);
    const right_val = try evaluateExpression(allocator, right_raw, variables);
    defer allocator.free(right_val);

    const left_num = std.fmt.parseInt(i64, left_val, 10) catch null;
    const right_num = std.fmt.parseInt(i64, right_val, 10) catch null;

    if (left_num != null and right_num != null) {
        if (std.mem.eql(u8, op, ">")) return left_num.? > right_num.?;
        if (std.mem.eql(u8, op, "<")) return left_num.? < right_num.?;
        if (std.mem.eql(u8, op, ">=")) return left_num.? >= right_num.?;
        if (std.mem.eql(u8, op, "<=")) return left_num.? <= right_num.?;
        if (std.mem.eql(u8, op, "==")) return left_num.? == right_num.?;
        if (std.mem.eql(u8, op, "!=")) return !std.mem.eql(u8, left_val, right_val);
    } else {
        if (std.mem.eql(u8, op, "==")) return std.mem.eql(u8, left_val, right_val);
        if (std.mem.eql(u8, op, "!=")) return !std.mem.eql(u8, left_val, right_val);
    }

    return false;
}

fn splitBraceSafe(allocator: std.mem.Allocator, content: []const u8, separator: u8) ![][]const u8 {
    var res: std.ArrayList([]const u8) = .empty;
    errdefer res.deinit(allocator);
    var depth: i32 = 0;
    var start: usize = 0;
    var i: usize = 0;
    while (i < content.len) : (i += 1) {
        const char = content[i];
        if (char == '{' or char == '[') depth += 1;
        if (char == '}' or char == ']') depth -= 1;
        if (char == separator and depth == 0) {
            try res.append(allocator, content[start..i]);
            start = i + 1;
        }
    }
    try res.append(allocator, content[start..]);
    return try res.toOwnedSlice(allocator);
}

fn parseRecursive(allocator: std.mem.Allocator, io: std.Io, variables: *std.StringHashMap(Variable), allocated_keys: *std.ArrayList([]const u8), prefix: []const u8, val_str: []const u8, is_mutable: bool, path: []const u8, line_idx: usize) anyerror![]const u8 {
    const trimmed = std.mem.trim(u8, val_str, " \t\r\n");
    if (trimmed.len == 0) return try allocator.dupe(u8, "");

    if (std.mem.startsWith(u8, trimmed, "{") and std.mem.endsWith(u8, trimmed, "}")) {
        const content = std.mem.trim(u8, trimmed[1 .. trimmed.len - 1], " \t\r\n");
        const entries = try splitBraceSafe(allocator, content, ',');
        defer allocator.free(entries);
        
        var obj_buf: std.ArrayList(u8) = .empty;
        errdefer obj_buf.deinit(allocator);
        try obj_buf.append(allocator, '{');
        var first = true;
        for (entries) |entry| {
            const trimmed_entry = std.mem.trim(u8, entry, " \t\r\n");
            if (trimmed_entry.len == 0) continue;
            if (std.mem.indexOf(u8, trimmed_entry, ":")) |colon_idx| {
                const key = std.mem.trim(u8, trimmed_entry[0..colon_idx], " \t\r\n");
                const v_expr = std.mem.trim(u8, trimmed_entry[colon_idx + 1 ..], " \t\r\n");
                const full_key = if (prefix.len > 0) try std.fmt.allocPrint(allocator, "{s}.{s}", .{ prefix, key }) else try allocator.dupe(u8, key);
                errdefer allocator.free(full_key);
                try allocated_keys.append(allocator, full_key);
                const v = try parseRecursive(allocator, io, variables, allocated_keys, full_key, v_expr, is_mutable, path, line_idx);
                
                var formatted_v: []const u8 = undefined;
                if (std.fmt.parseInt(i64, v, 10) catch null) |_| {
                    formatted_v = try allocator.dupe(u8, v);
                } else if (std.mem.eql(u8, v, "true") or std.mem.eql(u8, v, "false")) {
                    formatted_v = try allocator.dupe(u8, v);
                } else if (std.mem.startsWith(u8, v, "{") or std.mem.startsWith(u8, v, "[") or std.mem.startsWith(u8, v, "[object")) {
                    formatted_v = try allocator.dupe(u8, v);
                } else {
                    formatted_v = try std.fmt.allocPrint(allocator, "\"{s}\"", .{v});
                }
                defer allocator.free(formatted_v);

                if (!first) {
                    try obj_buf.append(allocator, ',');
                }
                try obj_buf.append(allocator, '"');
                try obj_buf.appendSlice(allocator, key);
                try obj_buf.appendSlice(allocator, "\":");
                try obj_buf.appendSlice(allocator, formatted_v);
                
                if (variables.get(full_key)) |old| allocator.free(old.value);
                try variables.put(full_key, .{ 
                    .value = v, 
                    .is_mutable = is_mutable,
                    .decl_line = 0, // Simplified for now
                    .decl_path = "",
                });
                first = false;
            }
        }
        try obj_buf.append(allocator, '}');
        return try obj_buf.toOwnedSlice(allocator);
    } else if (std.mem.startsWith(u8, trimmed, "[") and std.mem.endsWith(u8, trimmed, "]")) {
        const content = std.mem.trim(u8, trimmed[1 .. trimmed.len - 1], " \t\r\n");
        const entries = try splitBraceSafe(allocator, content, ',');
        defer allocator.free(entries);
        
        var list_buf: std.ArrayList(u8) = .empty;
        defer list_buf.deinit(allocator);
        try list_buf.append(allocator, '[');
        var idx: usize = 0;
        for (entries) |elem| {
            const trimmed_elem = std.mem.trim(u8, elem, " \t\r\n");
            if (trimmed_elem.len == 0) continue;
            if (idx > 0) {
                try list_buf.appendSlice(allocator, ",");
            }
            const full_key = try std.fmt.allocPrint(allocator, "{s}.{d}", .{ prefix, idx });
            errdefer allocator.free(full_key);
            try allocated_keys.append(allocator, full_key);
            const v = try parseRecursive(allocator, io, variables, allocated_keys, full_key, trimmed_elem, is_mutable, path, line_idx);
            
            var formatted_v: []const u8 = undefined;
            if (std.fmt.parseFloat(f64, v) catch null) |_| {
                formatted_v = try allocator.dupe(u8, v);
            } else if (std.mem.eql(u8, v, "true") or std.mem.eql(u8, v, "false")) {
                formatted_v = try allocator.dupe(u8, v);
            } else if (std.mem.startsWith(u8, v, "{") or std.mem.startsWith(u8, v, "[") or std.mem.startsWith(u8, v, "[object")) {
                formatted_v = try allocator.dupe(u8, v);
            } else {
                formatted_v = try std.fmt.allocPrint(allocator, "\"{s}\"", .{v});
            }
            defer allocator.free(formatted_v);

            try list_buf.appendSlice(allocator, formatted_v);
            if (variables.get(full_key)) |old| allocator.free(old.value);
            try variables.put(full_key, .{ 
                .value = v, 
                .is_mutable = is_mutable,
                .decl_line = 0,
                .decl_path = "",
            });
            idx += 1;
        }
        try list_buf.append(allocator, ']');
        return try list_buf.toOwnedSlice(allocator);
    } else {
        return try evaluateExpression(allocator, trimmed, variables.*);
    }
}

fn executeStatements(allocator: std.mem.Allocator, io: std.Io, lines: [][]const u8, variables: *std.StringHashMap(Variable), allocated_keys: *std.ArrayList([]const u8), if_was_executed: *bool, routes: *std.ArrayList(Route), path: []const u8, line_offset: usize) anyerror!void {
    var i: usize = 0;
    while (i < lines.len) : (i += 1) {
        const line = lines[i];
        var line_trimmed = std.mem.trim(u8, line, " \t\r");
        if (line_trimmed.len == 0) continue;
        if (std.mem.endsWith(u8, line_trimmed, ";")) {
            line_trimmed = std.mem.trim(u8, line_trimmed[0 .. line_trimmed.len - 1], " \t\r");
        }
        const trimmed = line_trimmed;

        if (std.mem.startsWith(u8, trimmed, "for ")) {
            const in_idx = std.mem.indexOf(u8, trimmed, " in ") orelse continue;
            const dotdot_idx = std.mem.indexOf(u8, trimmed, "..") orelse continue;
            const brace_idx = std.mem.indexOf(u8, trimmed, "{") orelse continue;

            const var_name = std.mem.trim(u8, trimmed[4..in_idx], " \t");
            const start_expr = std.mem.trim(u8, trimmed[in_idx + 4 .. dotdot_idx], " \t");
            const end_expr = std.mem.trim(u8, trimmed[dotdot_idx + 2 .. brace_idx], " \t");

            const start_val_s = try evaluateExpression(allocator, start_expr, variables.*);
            defer allocator.free(start_val_s);
            const end_val_s = try evaluateExpression(allocator, end_expr, variables.*);
            defer allocator.free(end_val_s);

            const start_val = std.fmt.parseInt(i64, start_val_s, 10) catch 0;
            const end_val = std.fmt.parseInt(i64, end_val_s, 10) catch 0;

            const block_end = findClosingBrace(lines, i);
            const block_lines = lines[i + 1 .. block_end];

            var loop_val = start_val;
            while (loop_val <= end_val) : (loop_val += 1) {
                const loop_val_str = try std.fmt.allocPrint(allocator, "{d}", .{loop_val});
                if (variables.get(var_name)) |old| allocator.free(old.value);
                try variables.put(var_name, .{ .value = loop_val_str, .is_mutable = true });

                var dummy_if = false;
                try executeStatements(allocator, io, block_lines, variables, allocated_keys, &dummy_if, routes, path, line_offset + i + 1);
            }
            i = block_end;
            if_was_executed.* = false;
        } else if (std.mem.startsWith(u8, trimmed, "if")) {
            const start_p = std.mem.indexOf(u8, trimmed, "(") orelse continue;
            const end_p = std.mem.lastIndexOf(u8, trimmed, ")") orelse continue;
            const cond_str = trimmed[start_p + 1 .. end_p];
            const cond_result = try evaluateCondition(allocator, cond_str, variables.*);

            const block_end = findClosingBrace(lines, i);
            const block_lines = lines[i + 1 .. block_end];

            if (cond_result) {
                var dummy_if = false;
                try executeStatements(allocator, io, block_lines, variables, allocated_keys, &dummy_if, routes, path, line_offset + i + 1);
                if_was_executed.* = true;
            } else {
                if_was_executed.* = false;
            }
            i = block_end;
        } else if (std.mem.startsWith(u8, trimmed, "else")) {
            const block_end = findClosingBrace(lines, i);
            const block_lines = lines[i + 1 .. block_end];

            if (!if_was_executed.*) {
                var dummy_if = false;
                try executeStatements(allocator, io, block_lines, variables, allocated_keys, &dummy_if, routes, path, line_offset + i + 1);
            }
            i = block_end;
            if_was_executed.* = false;
        } else if (std.mem.startsWith(u8, trimmed, "print(")) {
            const open_p = std.mem.indexOf(u8, trimmed, "(") orelse continue;
            const close_p = std.mem.lastIndexOf(u8, trimmed, ")") orelse continue;
            const arg = std.mem.trim(u8, trimmed[open_p + 1 .. close_p], " \t");

            if (std.mem.startsWith(u8, arg, "\"") and std.mem.endsWith(u8, arg, "\"")) {
                const content_to_print = arg[1 .. arg.len - 1];
                var start_idx: usize = 0;
                while (std.mem.indexOfPos(u8, content_to_print, start_idx, "{")) |open_brace| {
                    if (std.mem.indexOfPos(u8, content_to_print, open_brace, "}")) |close_brace| {
                        std.debug.print("{s}", .{content_to_print[start_idx..open_brace]});
                        const expr = content_to_print[open_brace + 1 .. close_brace];
                        const val = try evaluateExpression(allocator, expr, variables.*);
                        defer allocator.free(val);
                        std.debug.print("{s}", .{val});
                        start_idx = close_brace + 1;
                    } else {
                        break;
                    }
                }
                std.debug.print("{s}\n", .{content_to_print[start_idx..]});
            } else {
                const val = try evaluateExpression(allocator, arg, variables.*);
                defer allocator.free(val);
                std.debug.print("{s}\n", .{val});
            }
            if_was_executed.* = false;

        } else if (std.mem.startsWith(u8, trimmed, "return ")) {
            return;
        } else if (std.mem.indexOf(u8, trimmed, ".")) |dot_idx| {
            if (std.mem.indexOfPos(u8, trimmed, dot_idx, "(")) |open_p| {
                const var_name_raw = std.mem.trim(u8, trimmed[0..dot_idx], " \t");
                const var_name = try normalizeKey(allocator, var_name_raw);
                try allocated_keys.append(allocator, var_name);
                const method_name = std.mem.trim(u8, trimmed[dot_idx + 1 .. open_p], " \t");

                const methods = [_][]const u8{ "get", "post", "put", "delete", "patch" };
                var found_method = false;
                for (methods) |m| {
                    if (std.mem.eql(u8, method_name, m)) {
                        found_method = true;
                        const close_line_idx = findClosingBrace(lines, i);
                        const first_line = trimmed;
                        const comma_idx = std.mem.indexOf(u8, first_line, ",") orelse first_line.len;
                        const path_raw = std.mem.trim(u8, first_line[open_p + 1 .. comma_idx], " \t'\"");
                        
                        const handler_lines = try allocator.alloc([]const u8, close_line_idx - (i + 1));
                        errdefer allocator.free(handler_lines);
                        for (handler_lines, 0..) |*hl, hli| {
                            hl.* = try allocator.dupe(u8, lines[i + 1 + hli]);
                        }
                        const method_upper = try allocator.alloc(u8, m.len);
                        _ = std.ascii.upperString(method_upper, m);
                        try allocated_keys.append(allocator, method_upper);

                        try routes.append(allocator, .{
                            .method = method_upper,
                            .path = try allocator.dupe(u8, path_raw),
                            .handler_lines = handler_lines,
                        });
                        i = close_line_idx;
                        break;
                    }
                }

                if (!found_method) {
                    const close_p = std.mem.lastIndexOf(u8, trimmed, ")") orelse 0;
                    if (close_p > open_p) {
                        const arg_str = std.mem.trim(u8, trimmed[open_p + 1 .. close_p], " \t");
                        if (std.mem.eql(u8, method_name, "push")) {
                            if (variables.get(var_name)) |v| {
                                const val = v.value;
                                const val_trimmed = std.mem.trim(u8, val, " \t");
                                if (std.mem.startsWith(u8, val_trimmed, "[") and std.mem.endsWith(u8, val_trimmed, "]")) {
                                     const inner = std.mem.trim(u8, val_trimmed[1 .. val_trimmed.len - 1], " \t");
                                    var count: usize = 0;
                                    if (inner.len > 0) {
                                        const elems = try splitBraceSafe(allocator, inner, ',');
                                        count = elems.len;
                                        allocator.free(elems);
                                    }
                                    
                                    const prefix = try std.fmt.allocPrint(allocator, "{s}.{d}", .{var_name, count});
                                    try allocated_keys.append(allocator, prefix);
                                    const arg_val = try parseRecursive(allocator, io, variables, allocated_keys, prefix, arg_str, v.is_mutable, path, line_offset + i);
                                    
                                    var new_val: []u8 = undefined;
                                    if (inner.len == 0) {
                                        new_val = try std.fmt.allocPrint(allocator, "[{s}]", .{arg_val});
                                    } else {
                                        new_val = try std.fmt.allocPrint(allocator, "[{s},{s}]", .{inner, arg_val});
                                    }
                                    
                                    if (variables.get(prefix)) |old| allocator.free(old.value);
                                    try variables.put(prefix, .{ .value = arg_val, .is_mutable = v.is_mutable, .decl_line = line_offset + i, .decl_path = path });

                                    if (variables.get(var_name)) |old| allocator.free(old.value);
                                    try variables.put(var_name, .{ .value = new_val, .is_mutable = v.is_mutable, .decl_line = line_offset + i, .decl_path = path });
                                }
                            }
                        } else if (std.mem.eql(u8, method_name, "pop")) {
                            if (variables.get(var_name)) |v| {
                                const val = v.value;
                                const val_trimmed = std.mem.trim(u8, val, " \t");
                                if (std.mem.startsWith(u8, val_trimmed, "[") and std.mem.endsWith(u8, val_trimmed, "]")) {
                                    const inner = std.mem.trim(u8, val_trimmed[1 .. val_trimmed.len - 1], " \t");
                                    var new_val: []u8 = undefined;
                                    const elems = try splitBraceSafe(allocator, inner, ',');
                                    if (elems.len > 0) {
                                        var new_inner_buf: std.ArrayList(u8) = .empty;
                                        defer new_inner_buf.deinit(allocator);
                                        for (elems[0 .. elems.len - 1], 0..) |elem, idx| {
                                            if (idx > 0) try new_inner_buf.appendSlice(allocator, ",");
                                            try new_inner_buf.appendSlice(allocator, elem);
                                        }
                                        new_val = try std.fmt.allocPrint(allocator, "[{s}]", .{new_inner_buf.items});
                                    } else {
                                        new_val = try allocator.dupe(u8, "[]");
                                    }
                                    allocator.free(elems);
                                    if (variables.get(var_name)) |old| allocator.free(old.value);
                                    try variables.put(var_name, .{ .value = new_val, .is_mutable = v.is_mutable });
                                }
                            }
                        }
                    }
                }
                if_was_executed.* = false;
            }
        } else if (std.mem.indexOf(u8, trimmed, "=")) |index| {
            const is_comparison = if (index + 1 < trimmed.len and trimmed[index + 1] == '=')
                true
            else if (index > 0 and (trimmed[index - 1] == '!' or trimmed[index - 1] == '>' or trimmed[index - 1] == '<'))
                true
            else
                false;

            if (!is_comparison) {
                var is_decl = false;
                var current_is_mutable = true;

                var decl_part = std.mem.trim(u8, trimmed[0..index], " \t");
                if (std.mem.startsWith(u8, decl_part, "let ")) {
                    decl_part = std.mem.trim(u8, decl_part[4..], " \t");
                    is_decl = true;
                    current_is_mutable = true;
                } else if (std.mem.startsWith(u8, decl_part, "const ")) {
                    decl_part = std.mem.trim(u8, decl_part[6..], " \t");
                    is_decl = true;
                    current_is_mutable = false;
                }

                var var_name_raw = decl_part;
                if (std.mem.indexOf(u8, decl_part, ":")) |colon_idx| {
                    var_name_raw = std.mem.trim(u8, decl_part[0..colon_idx], " \t");
                }
                const var_name = try normalizeKey(allocator, var_name_raw);
                errdefer allocator.free(var_name);

                if (!is_decl) {
                    if (variables.get(var_name)) |old_v| {
                        if (!old_v.is_mutable) {
                            const msg = try std.fmt.allocPrint(allocator, "Cannot assign to \"{s}\" because it is a constant", .{var_name});
                            defer allocator.free(msg);
                            const note_msg = try std.fmt.allocPrint(allocator, "The symbol \"{s}\" was declared a constant here:", .{var_name});
                            defer allocator.free(note_msg);
                            
                            printPrettyError(allocator, io, path, lines, line_offset + i, msg, old_v.decl_path, old_v.decl_line, note_msg, var_name);
                            return error.ImmutableVariable;
                        }
                        current_is_mutable = old_v.is_mutable;
                    }
                }

                try allocated_keys.append(allocator, var_name);

                const val_raw = std.mem.trim(u8, trimmed[index + 1 ..], " \t");
                
                if (std.mem.startsWith(u8, val_raw, "serve(") or std.mem.startsWith(u8, val_raw, "route(")) {
                    var port_val: []const u8 = try allocator.dupe(u8, "3000");
                    if (std.mem.indexOf(u8, val_raw, "port:")) |p_idx| {
                        allocator.free(port_val);
                        const rest = val_raw[p_idx + 5 ..];
                        const end_idx = std.mem.indexOfAny(u8, rest, "},)") orelse rest.len;
                        port_val = try allocator.dupe(u8, std.mem.trim(u8, rest[0..end_idx], " \t\""));
                    } else {
                        var j = i + 1;
                        while (j < lines.len) : (j += 1) {
                            const s_line = std.mem.trim(u8, lines[j], " \t\r");
                            if (std.mem.indexOf(u8, s_line, "port:")) |p_idx| {
                                allocator.free(port_val);
                                const rest = s_line[p_idx + 5 ..];
                                const end_idx = std.mem.indexOfAny(u8, rest, "},)") orelse rest.len;
                                port_val = try allocator.dupe(u8, std.mem.trim(u8, rest[0..end_idx], " \t\""));
                                break;
                            }
                            if (std.mem.indexOf(u8, s_line, "})") != null or std.mem.indexOf(u8, s_line, ")") != null) break;
                        }
                    }
                    try variables.put("__server_port__", .{ .value = port_val, .is_mutable = true });
                    if (variables.get(var_name)) |old| allocator.free(old.value);
                    try variables.put(var_name, .{ .value = try allocator.dupe(u8, "[object Server]"), .is_mutable = true });
                } else if (std.mem.startsWith(u8, val_raw, "{")) {
                    const block_end = findClosingBrace(lines, i);
                    var obj_buf: std.ArrayList(u8) = .empty;
                    defer obj_buf.deinit(allocator);
                    try obj_buf.appendSlice(allocator, val_raw);
                    var j = i + 1;
                    while (j <= block_end) : (j += 1) {
                        try obj_buf.append(allocator, '\n');
                        try obj_buf.appendSlice(allocator, lines[j]);
                    }
                    i = block_end;
                    const v = try parseRecursive(allocator, io, variables, allocated_keys, var_name, obj_buf.items, current_is_mutable, path, line_offset + i);
                    if (variables.get(var_name)) |old| allocator.free(old.value);
                    try variables.put(var_name, .{ .value = v, .is_mutable = current_is_mutable });
                } else if (std.mem.startsWith(u8, val_raw, "[")) {
                    const block_end = findClosingBrace(lines, i);
                    var list_buf: std.ArrayList(u8) = .empty;
                    defer list_buf.deinit(allocator);
                    try list_buf.appendSlice(allocator, val_raw);
                    var j = i + 1;
                    while (j <= block_end) : (j += 1) {
                        try list_buf.append(allocator, '\n');
                        try list_buf.appendSlice(allocator, lines[j]);
                    }
                    i = block_end;
                    const v = try parseRecursive(allocator, io, variables, allocated_keys, var_name, list_buf.items, current_is_mutable, path, line_offset + i);
                    if (variables.get(var_name)) |old| allocator.free(old.value);
                    try variables.put(var_name, .{ .value = v, .is_mutable = current_is_mutable });
                } else {
                    const value_to_store = try evaluateExpression(allocator, val_raw, variables.*);
                    if (variables.get(var_name)) |old_v| {
                        allocator.free(old_v.value);
                    }
                    try variables.put(var_name, .{ 
                        .value = value_to_store, 
                        .is_mutable = current_is_mutable,
                        .decl_line = line_offset + i,
                        .decl_path = path,
                    });
                }
                if_was_executed.* = false;
            }
        }
    }
}
