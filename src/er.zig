const std = @import("std");

const Route = struct {
    method: []const u8,
    path: []const u8,
    handler_lines: [][]const u8,
};

pub fn runFile(allocator: std.mem.Allocator, io: std.Io, path: []const u8) !void {
    const content = try std.Io.Dir.cwd().readFileAlloc(io, path, allocator, @enumFromInt(1024 * 1024));
    defer allocator.free(content);

    var variables = std.StringHashMap([]const u8).init(allocator);
    var allocated_keys: std.ArrayList([]const u8) = .empty;
    defer {
        var it = variables.valueIterator();
        while (it.next()) |val| {
            allocator.free(val.*);
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
        }
        routes.deinit(allocator);
    }
    try executeStatements(allocator, lines, &variables, &allocated_keys, &if_was_executed, &routes);

    if (variables.get("__server_port__")) |port_s| {
        const port = std.fmt.parseInt(u16, port_s, 10) catch 3000;
        try startServer(allocator, io, port, &routes, &variables);
    }
}

pub fn handleApiRequest(allocator: std.mem.Allocator, io: std.Io, request: *std.http.Server.Request, api_file_path: []const u8, api_prefix: []const u8) !bool {
    const content = std.Io.Dir.cwd().readFileAlloc(io, api_file_path, allocator, @enumFromInt(1024 * 1024)) catch return false;
    defer allocator.free(content);

    var variables = std.StringHashMap([]const u8).init(allocator);
    var allocated_keys: std.ArrayList([]const u8) = .empty;
    defer {
        var it = variables.valueIterator();
        while (it.next()) |val| {
            allocator.free(val.*);
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
        }
        routes.deinit(allocator);
    }
    try executeStatements(allocator, lines, &variables, &allocated_keys, &if_was_executed, &routes);

    const target = request.head.target;
    var clean_target = target;
    if (std.mem.indexOfScalar(u8, clean_target, '?')) |idx| clean_target = clean_target[0..idx];
    if (std.mem.indexOfScalar(u8, clean_target, '#')) |idx| clean_target = clean_target[0..idx];

    // Check if target starts with api_prefix
    if (!std.mem.startsWith(u8, clean_target, api_prefix)) return false;
    const sub_path = clean_target[api_prefix.len..];

    for (routes.items) |route| {
        if (std.mem.eql(u8, route.method, @tagName(request.head.method))) {
            var match = std.mem.eql(u8, route.path, sub_path);
            if (!match) {
                if (std.mem.eql(u8, route.path, "/") and sub_path.len == 0) {
                    match = true;
                } else if (sub_path.len > 0 and sub_path[0] == '/') {
                    if (std.mem.eql(u8, route.path, sub_path[1..])) match = true;
                } else if (route.path.len > 0 and route.path[0] == '/') {
                    if (std.mem.eql(u8, route.path[1..], sub_path)) match = true;
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

fn startServer(allocator: std.mem.Allocator, io: std.Io, port: u16, routes: *std.ArrayList(Route), variables: *std.StringHashMap([]const u8)) !void {
    const address = std.Io.net.IpAddress.parse("0.0.0.0", port) catch try std.Io.net.IpAddress.parse("127.0.0.1", port);
    var server = try address.listen(io, .{ .reuse_address = true });
    defer server.deinit(io);

    std.debug.print("lisinning port {d}\n", .{port});

    while (true) {
        const conn = try server.accept(io);
        defer conn.close(io);

        var reader_buf: [4096]u8 = undefined;
        var reader = conn.reader(io, &reader_buf);
        var writer_buf: [4096]u8 = undefined;
        var writer = conn.writer(io, &writer_buf);

        var http_server = std.http.Server.init(&reader.interface, &writer.interface);
        var request = http_server.receiveHead() catch continue;

        var matched = false;
        for (routes.items) |route| {
            if (std.mem.eql(u8, route.path, request.head.target)) {
                for (route.handler_lines) |h_line| {
                    const h_trimmed = std.mem.trim(u8, h_line, " \t\r");
                    if (std.mem.indexOf(u8, h_trimmed, "c.json(")) |json_idx| {
                        const o_p = std.mem.indexOfPos(u8, h_trimmed, json_idx, "(") orelse continue;
                        const c_p = std.mem.lastIndexOf(u8, h_trimmed, ")") orelse continue;
                        const data_expr = h_trimmed[o_p + 1 .. c_p];
                        const data_val = try evaluateExpression(allocator, data_expr, variables.*);
                        defer allocator.free(data_val);

                        _ = try request.respond(data_val, .{
                            .status = .ok,
                            .extra_headers = &.{.{ .name = "Content-Type", .value = "application/json" }},
                        });
                        matched = true;
                        break;
                    }
                }
            }
            if (matched) break;
        }

        if (!matched) {
            _ = try request.respond("Not Found", .{ .status = .not_found });
        }
    }
}

fn findClosingBrace(lines: [][]const u8, start_idx: usize) usize {
    var depth: usize = 0;
    var i = start_idx;
    while (i < lines.len) : (i += 1) {
        const trimmed = std.mem.trim(u8, lines[i], " \t\r");
        for (trimmed) |char| {
            if (char == '{') depth += 1;
            if (char == '}') {
                if (depth > 0) {
                    depth -= 1;
                    if (depth == 0) return i;
                }
            }
        }
    }
    return i;
}

fn normalizeKey(allocator: std.mem.Allocator, key: []const u8) anyerror![]const u8 {
    if (std.mem.indexOfScalar(u8, key, '[') == null and std.mem.indexOfScalar(u8, key, ']') == null) {
        return try allocator.dupe(u8, key);
    }
    var res: std.ArrayList(u8) = .empty;
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

fn evaluateExpression(allocator: std.mem.Allocator, expr: []const u8, variables: std.StringHashMap([]const u8)) anyerror![]const u8 {
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

    if (variables.get(trimmed)) |val| {
        return try allocator.dupe(u8, val);
    }
    return try allocator.dupe(u8, trimmed);
}

fn evaluateCondition(allocator: std.mem.Allocator, condition: []const u8, variables: std.StringHashMap([]const u8)) anyerror!bool {
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
        if (std.mem.eql(u8, op, "!=")) return left_num.? != right_num.?;
    } else {
        if (std.mem.eql(u8, op, "==")) return std.mem.eql(u8, left_val, right_val);
        if (std.mem.eql(u8, op, "!=")) return !std.mem.eql(u8, left_val, right_val);
    }

    return false;
}

fn parseRecursive(allocator: std.mem.Allocator, variables: *std.StringHashMap([]const u8), allocated_keys: *std.ArrayList([]const u8), prefix: []const u8, val_str: []const u8) anyerror![]const u8 {
    const trimmed = std.mem.trim(u8, val_str, " \t\r");
    if (trimmed.len == 0) return try allocator.dupe(u8, "");

    if (std.mem.startsWith(u8, trimmed, "{") and std.mem.endsWith(u8, trimmed, "}")) {
        const content = std.mem.trim(u8, trimmed[1 .. trimmed.len - 1], " \t");
        var it = std.mem.splitSequence(u8, content, ",");
        var obj_buf: std.ArrayList(u8) = .empty;
        defer obj_buf.deinit(allocator);
        try obj_buf.append(allocator, '{');
        var first = true;
        while (it.next()) |entry| {
            if (std.mem.indexOf(u8, entry, ":")) |colon_idx| {
                const key = std.mem.trim(u8, entry[0..colon_idx], " \t");
                const v_expr = std.mem.trim(u8, entry[colon_idx + 1 ..], " \t");
                const full_key = try std.fmt.allocPrint(allocator, "{s}.{s}", .{ prefix, key });
                try allocated_keys.append(allocator, full_key);
                const v = try parseRecursive(allocator, variables, allocated_keys, full_key, v_expr);
                
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
                    try obj_buf.appendSlice(allocator, ",\n    ");
                } else {
                    try obj_buf.appendSlice(allocator, "\n    ");
                }
                try obj_buf.append(allocator, '"');
                try obj_buf.appendSlice(allocator, key);
                try obj_buf.appendSlice(allocator, "\": ");
                try obj_buf.appendSlice(allocator, formatted_v);
                
                if (variables.get(full_key)) |old| allocator.free(old);
                try variables.put(full_key, v);
                first = false;
            }
        }
        if (!first) {
            try obj_buf.appendSlice(allocator, "\n  }");
        } else {
            try obj_buf.append(allocator, '}');
        }
        return try obj_buf.toOwnedSlice(allocator);
    } else if (std.mem.startsWith(u8, trimmed, "[") and std.mem.endsWith(u8, trimmed, "]")) {
        const content = std.mem.trim(u8, trimmed[1 .. trimmed.len - 1], " \t");
        var it = std.mem.splitSequence(u8, content, ",");
        var list_buf: std.ArrayList(u8) = .empty;
        defer list_buf.deinit(allocator);
        try list_buf.append(allocator, '[');
        var idx: usize = 0;
        while (it.next()) |elem| {
            const trimmed_elem = std.mem.trim(u8, elem, " \t");
            if (trimmed_elem.len == 0) continue;
            if (idx > 0) {
                try list_buf.appendSlice(allocator, ",\n  ");
            } else {
                try list_buf.appendSlice(allocator, "\n  ");
            }
            const full_key = try std.fmt.allocPrint(allocator, "{s}.{d}", .{ prefix, idx });
            try allocated_keys.append(allocator, full_key);
            const v = try parseRecursive(allocator, variables, allocated_keys, full_key, trimmed_elem);
            
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

            try list_buf.appendSlice(allocator, formatted_v);
            if (variables.get(full_key)) |old| allocator.free(old);
            try variables.put(full_key, v);
            idx += 1;
        }
        if (idx > 0) {
            try list_buf.appendSlice(allocator, "\n]");
        } else {
            try list_buf.append(allocator, ']');
        }
        return try list_buf.toOwnedSlice(allocator);
    } else {
        return try evaluateExpression(allocator, trimmed, variables.*);
    }
}

fn executeStatements(allocator: std.mem.Allocator, lines: [][]const u8, variables: *std.StringHashMap([]const u8), allocated_keys: *std.ArrayList([]const u8), if_was_executed: *bool, routes: *std.ArrayList(Route)) anyerror!void {
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
                if (variables.get(var_name)) |old| allocator.free(old);
                try variables.put(var_name, loop_val_str);

                var dummy_if = false;
                try executeStatements(allocator, block_lines, variables, allocated_keys, &dummy_if, routes);
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
                try executeStatements(allocator, block_lines, variables, allocated_keys, &dummy_if, routes);
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
                try executeStatements(allocator, block_lines, variables, allocated_keys, &dummy_if, routes);
            }
            i = block_end;
            if_was_executed.* = false;
        } else if (std.mem.startsWith(u8, trimmed, "print(")) {
            const open_p = std.mem.indexOf(u8, trimmed, "(") orelse continue;
            const close_p = std.mem.lastIndexOf(u8, trimmed, ")") orelse {
                std.debug.print("Syntax Error: Missing ')'\n", .{});
                continue;
            };
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
        } else if (std.mem.startsWith(u8, trimmed, "get(")) {
            const open_p = std.mem.indexOf(u8, trimmed, "(") orelse continue;
            const close_line_idx = findClosingBrace(lines, i);
            const first_line = trimmed;
            const comma_idx = std.mem.indexOf(u8, first_line, ",") orelse first_line.len;
            const path_raw = std.mem.trim(u8, first_line[open_p + 1 .. comma_idx], " \t'\"");
            
            const handler_lines = lines[i + 1 .. close_line_idx];
            try routes.append(allocator, .{
                .method = "GET",
                .path = try allocator.dupe(u8, path_raw),
                .handler_lines = handler_lines,
            });
            i = close_line_idx;
            if_was_executed.* = false;
        } else if (std.mem.startsWith(u8, trimmed, "post(")) {
            const open_p = std.mem.indexOf(u8, trimmed, "(") orelse continue;
            const close_line_idx = findClosingBrace(lines, i);
            const first_line = trimmed;
            const comma_idx = std.mem.indexOf(u8, first_line, ",") orelse first_line.len;
            const path_raw = std.mem.trim(u8, first_line[open_p + 1 .. comma_idx], " \t'\"");
            
            const handler_lines = lines[i + 1 .. close_line_idx];
            try routes.append(allocator, .{
                .method = "POST",
                .path = try allocator.dupe(u8, path_raw),
                .handler_lines = handler_lines,
            });
            i = close_line_idx;
            if_was_executed.* = false;
        } else if (std.mem.startsWith(u8, trimmed, "return ")) {
            return;
        } else if (std.mem.indexOf(u8, trimmed, ".")) |dot_idx| {
            if (std.mem.indexOfPos(u8, trimmed, dot_idx, "(")) |open_p| {
                const var_name_raw = std.mem.trim(u8, trimmed[0..dot_idx], " \t");
                const var_name = try normalizeKey(allocator, var_name_raw);
                try allocated_keys.append(allocator, var_name);
                const method_name = std.mem.trim(u8, trimmed[dot_idx + 1 .. open_p], " \t");

                if (std.mem.eql(u8, method_name, "get")) {
                    const close_line_idx = findClosingBrace(lines, i);
                    const first_line = trimmed;
                    const comma_idx = std.mem.indexOf(u8, first_line, ",") orelse first_line.len;
                    const path_raw = std.mem.trim(u8, first_line[open_p + 1 .. comma_idx], " \t'\"");
                    
                    const handler_lines = lines[i + 1 .. close_line_idx];
                    try routes.append(allocator, .{
                        .method = "GET",
                        .path = try allocator.dupe(u8, path_raw),
                        .handler_lines = handler_lines,
                    });
                    i = close_line_idx;
                } else if (std.mem.eql(u8, method_name, "post")) {
                    const close_line_idx = findClosingBrace(lines, i);
                    const first_line = trimmed;
                    const comma_idx = std.mem.indexOf(u8, first_line, ",") orelse first_line.len;
                    const path_raw = std.mem.trim(u8, first_line[open_p + 1 .. comma_idx], " \t'\"");
                    
                    const handler_lines = lines[i + 1 .. close_line_idx];
                    try routes.append(allocator, .{
                        .method = "POST",
                        .path = try allocator.dupe(u8, path_raw),
                        .handler_lines = handler_lines,
                    });
                    i = close_line_idx;
                } else {
                    const close_p = std.mem.lastIndexOf(u8, trimmed, ")") orelse 0;
                    if (close_p > open_p) {
                        const arg_str = std.mem.trim(u8, trimmed[open_p + 1 .. close_p], " \t");
                        if (std.mem.eql(u8, method_name, "push")) {
                            if (variables.get(var_name)) |val| {
                                const val_trimmed = std.mem.trim(u8, val, " \t");
                                if (std.mem.startsWith(u8, val_trimmed, "[") and std.mem.endsWith(u8, val_trimmed, "]")) {
                                     const inner = std.mem.trim(u8, val_trimmed[1 .. val_trimmed.len - 1], " \t");
                                    var count: usize = 0;
                                    if (inner.len > 0) {
                                        var it = std.mem.splitSequence(u8, inner, ",");
                                        while (it.next()) |_| count += 1;
                                    }
                                    
                                    const prefix = try std.fmt.allocPrint(allocator, "{s}.{d}", .{var_name, count});
                                    try allocated_keys.append(allocator, prefix);
                                    const arg_val = try parseRecursive(allocator, variables, allocated_keys, prefix, arg_str);
                                    
                                    var new_val: []u8 = undefined;
                                    if (inner.len == 0) {
                                        new_val = try std.fmt.allocPrint(allocator, "[{s}]", .{arg_val});
                                    } else {
                                        new_val = try std.fmt.allocPrint(allocator, "[{s}, {s}]", .{inner, arg_val});
                                    }
                                    
                                    if (variables.get(prefix)) |old| allocator.free(old);
                                    try variables.put(prefix, arg_val);

                                    if (variables.get(var_name)) |old| allocator.free(old);
                                    try variables.put(var_name, new_val);
                                }
                            }
                        } else if (std.mem.eql(u8, method_name, "pop")) {
                            if (variables.get(var_name)) |val| {
                                const val_trimmed = std.mem.trim(u8, val, " \t");
                                if (std.mem.startsWith(u8, val_trimmed, "[") and std.mem.endsWith(u8, val_trimmed, "]")) {
                                    const inner = std.mem.trim(u8, val_trimmed[1 .. val_trimmed.len - 1], " \t");
                                    var new_val: []u8 = undefined;
                                    if (std.mem.lastIndexOf(u8, inner, ",")) |comma_idx| {
                                        const new_inner = std.mem.trim(u8, inner[0..comma_idx], " \t");
                                        new_val = try std.fmt.allocPrint(allocator, "[{s}]", .{new_inner});
                                    } else {
                                        new_val = try allocator.dupe(u8, "[]");
                                    }
                                    if (variables.get(var_name)) |old| allocator.free(old);
                                    try variables.put(var_name, new_val);
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
                var decl_part = std.mem.trim(u8, trimmed[0..index], " \t");
                if (std.mem.startsWith(u8, decl_part, "let ")) {
                    decl_part = std.mem.trim(u8, decl_part[4..], " \t");
                }

                var var_name_raw = decl_part;
                if (std.mem.indexOf(u8, decl_part, ":")) |colon_idx| {
                    var_name_raw = std.mem.trim(u8, decl_part[0..colon_idx], " \t");
                }
                const var_name = try normalizeKey(allocator, var_name_raw);
                try allocated_keys.append(allocator, var_name);

                const val_raw = std.mem.trim(u8, trimmed[index + 1 ..], " \t");
                
                if (std.mem.startsWith(u8, val_raw, "serve(")) {
                    var port_val: []const u8 = "3000";
                    if (std.mem.indexOf(u8, val_raw, "port:")) |p_idx| {
                        const rest = val_raw[p_idx + 5 ..];
                        const end_idx = std.mem.indexOfAny(u8, rest, "},)") orelse rest.len;
                        port_val = try allocator.dupe(u8, std.mem.trim(u8, rest[0..end_idx], " \t\""));
                    } else {
                        // Multi-line serve call?
                        var j = i + 1;
                        while (j < lines.len) : (j += 1) {
                            const s_line = std.mem.trim(u8, lines[j], " \t\r");
                            if (std.mem.indexOf(u8, s_line, "port:")) |p_idx| {
                                const rest = s_line[p_idx + 5 ..];
                                const end_idx = std.mem.indexOfAny(u8, rest, "},)") orelse rest.len;
                                port_val = try allocator.dupe(u8, std.mem.trim(u8, rest[0..end_idx], " \t\""));
                                break;
                            }
                            if (std.mem.indexOf(u8, s_line, "})") != null or std.mem.indexOf(u8, s_line, ")") != null) break;
                        }
                    }
                    try variables.put("__server_port__", port_val);
                    if (variables.get(var_name)) |old| allocator.free(old);
                    try variables.put(var_name, try allocator.dupe(u8, "[object Server]"));
                } else if (std.mem.startsWith(u8, val_raw, "{")) {
                    // Object literal
                    if (std.mem.endsWith(u8, val_raw, "}")) {
                        // Single line object literal
                        const v = try parseRecursive(allocator, variables, allocated_keys, var_name, val_raw);
                        if (variables.get(var_name)) |old| allocator.free(old);
                        try variables.put(var_name, v);
                    } else {
                        // Multi-line object literal
                        i += 1;
                        while (i < lines.len) : (i += 1) {
                            const obj_line = std.mem.trim(u8, lines[i], " \t\r");
                            if (std.mem.eql(u8, obj_line, "}")) break;
                            
                            const pair = if (std.mem.endsWith(u8, obj_line, ",")) obj_line[0..obj_line.len-1] else obj_line;
                            if (std.mem.indexOf(u8, pair, ":")) |colon_idx| {
                                const key = std.mem.trim(u8, pair[0..colon_idx], " \t");
                                const val_expr = std.mem.trim(u8, pair[colon_idx+1..], " \t");
                                const full_key = try std.fmt.allocPrint(allocator, "{s}.{s}", .{var_name, key});
                                try allocated_keys.append(allocator, full_key);
                                const val = try parseRecursive(allocator, variables, allocated_keys, full_key, val_expr);
                                if (variables.get(full_key)) |old| allocator.free(old);
                                try variables.put(full_key, val);
                            }
                        }
                        if (variables.get(var_name)) |old| allocator.free(old);
                        try variables.put(var_name, try allocator.dupe(u8, "[object Object]"));
                    }
                } else if (std.mem.startsWith(u8, val_raw, "[")) {
                    // List literal
                    var list_str: std.ArrayList(u8) = .empty;
                    defer list_str.deinit(allocator);
                    try list_str.append(allocator, '[');
                    
                    var idx: usize = 0;
                    if (std.mem.endsWith(u8, val_raw, "]")) {
                        // Single line list
                        const v = try parseRecursive(allocator, variables, allocated_keys, var_name, val_raw);
                        if (variables.get(var_name)) |old| allocator.free(old);
                        try variables.put(var_name, v);
                    } else {
                        // Multi-line list
                        i += 1;
                        while (i < lines.len) : (i += 1) {
                            const list_line = std.mem.trim(u8, lines[i], " \t\r");
                            if (std.mem.eql(u8, list_line, "]")) break;
                            const entry = if (std.mem.endsWith(u8, list_line, ",")) list_line[0..list_line.len-1] else list_line;
                            const trimmed_entry = std.mem.trim(u8, entry, " \t\r");
                            if (trimmed_entry.len == 0) continue;

                            if (idx > 0) {
                                try list_str.appendSlice(allocator, ",\n  ");
                            } else {
                                try list_str.appendSlice(allocator, "\n  ");
                            }
                            const prefix = try std.fmt.allocPrint(allocator, "{s}.{d}", .{var_name, idx});
                            try allocated_keys.append(allocator, prefix);
                            const val = try parseRecursive(allocator, variables, allocated_keys, prefix, trimmed_entry);
                            try list_str.appendSlice(allocator, val);
                            if (variables.get(prefix)) |old| allocator.free(old);
                            try variables.put(prefix, val);
                            idx += 1;
                        }
                        if (idx > 0) try list_str.appendSlice(allocator, "\n");
                        try list_str.append(allocator, ']');
                        if (variables.get(var_name)) |old| allocator.free(old);
                        try variables.put(var_name, try list_str.toOwnedSlice(allocator));
                    }
                } else {
                    const value_to_store = try evaluateExpression(allocator, val_raw, variables.*);
                    if (variables.get(var_name)) |old_val| {
                        allocator.free(old_val);
                    }
                    try variables.put(var_name, value_to_store);
                }
                if_was_executed.* = false;
            }
        }

    }
}
