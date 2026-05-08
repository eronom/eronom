const std = @import("std");

pub fn runFile(allocator: std.mem.Allocator, path: []const u8) !void {
    const file = std.fs.cwd().openFile(path, .{}) catch |err| {
        std.debug.print("Error opening file: {any}\n", .{err});
        return;
    };
    defer file.close();

    const content = try file.readToEndAlloc(allocator, 1024 * 1024);
    defer allocator.free(content);

    var variables = std.StringHashMap([]const u8).init(allocator);
    defer {
        var it = variables.valueIterator();
        while (it.next()) |val| {
            allocator.free(val.*);
        }
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
    try executeStatements(allocator, lines, &variables, &if_was_executed);
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

fn evaluateExpression(allocator: std.mem.Allocator, expr: []const u8, variables: std.StringHashMap([]const u8)) anyerror![]const u8 {
    const trimmed = std.mem.trim(u8, expr, " \t");
    if (trimmed.len == 0) return try allocator.dupe(u8, "");

    if (std.mem.startsWith(u8, trimmed, "\"") and std.mem.endsWith(u8, trimmed, "\"")) {
        return try allocator.dupe(u8, trimmed[1 .. trimmed.len - 1]);
    }

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

fn executeStatements(allocator: std.mem.Allocator, lines: [][]const u8, variables: *std.StringHashMap([]const u8), if_was_executed: *bool) anyerror!void {
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
                try executeStatements(allocator, block_lines, variables, &dummy_if);
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
                try executeStatements(allocator, block_lines, variables, &dummy_if);
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
                try executeStatements(allocator, block_lines, variables, &dummy_if);
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

                var var_name = decl_part;
                if (std.mem.indexOf(u8, decl_part, ":")) |colon_idx| {
                    var_name = std.mem.trim(u8, decl_part[0..colon_idx], " \t");
                    // Type part is decl_part[colon_idx + 1 ..]
                }

                const val_raw = std.mem.trim(u8, trimmed[index + 1 ..], " \t");
                const value_to_store = try evaluateExpression(allocator, val_raw, variables.*);

                if (variables.get(var_name)) |old_val| {
                    allocator.free(old_val);
                }
                try variables.put(var_name, value_to_store);
                if_was_executed.* = false;
            }
        } else if (std.mem.indexOf(u8, trimmed, ".")) |dot_idx| {
            if (std.mem.indexOfPos(u8, trimmed, dot_idx, "(")) |open_p| {
                const close_p = std.mem.lastIndexOf(u8, trimmed, ")") orelse 0;
                if (close_p > open_p) {
                    const var_name = std.mem.trim(u8, trimmed[0..dot_idx], " \t");
                    const method_name = std.mem.trim(u8, trimmed[dot_idx + 1 .. open_p], " \t");
                    const arg_str = std.mem.trim(u8, trimmed[open_p + 1 .. close_p], " \t");

                    if (std.mem.eql(u8, method_name, "push")) {
                        if (variables.get(var_name)) |val| {
                            const val_trimmed = std.mem.trim(u8, val, " \t");
                            if (std.mem.startsWith(u8, val_trimmed, "[") and std.mem.endsWith(u8, val_trimmed, "]")) {
                                const inner = std.mem.trim(u8, val_trimmed[1 .. val_trimmed.len - 1], " \t");
                                const arg_val = try evaluateExpression(allocator, arg_str, variables.*);
                                defer allocator.free(arg_val);

                                var new_val: []u8 = undefined;
                                if (inner.len == 0) {
                                    new_val = try std.fmt.allocPrint(allocator, "[{s}]", .{arg_val});
                                } else {
                                    new_val = try std.fmt.allocPrint(allocator, "[{s}, {s}]", .{inner, arg_val});
                                }

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
                    if_was_executed.* = false;
                }
            }
        }

    }
}
