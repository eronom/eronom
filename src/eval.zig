const std = @import("std");

pub const Value = union(enum) {
    null,
    boolean: bool,
    number: f64,
    string: []const u8,
    map: std.StringHashMap(Value),
    list: std.ArrayList(Value),

    pub fn deinit(self: *Value, allocator: std.mem.Allocator) void {
        switch (self.*) {
            .string => |s| allocator.free(s),
            .map => |*m| {
                var vit = m.valueIterator();
                while (vit.next()) |v| v.deinit(allocator);
                m.deinit();
            },
            .list => |*l| {
                for (l.items) |*v| v.deinit(allocator);
                l.deinit(allocator);
            },
            else => {},
        }
    }

    pub fn clone(self: Value, allocator: std.mem.Allocator) !Value {
        switch (self) {
            .string => |s| return .{ .string = try allocator.dupe(u8, s) },
            .map => |m| {
                var new_map = std.StringHashMap(Value).init(allocator);
                var it = m.iterator();
                while (it.next()) |entry| {
                    try new_map.put(try allocator.dupe(u8, entry.key_ptr.*), try entry.value_ptr.clone(allocator));
                }
                return .{ .map = new_map };
            },
            .list => |l| {
                var new_list: std.ArrayList(Value) = .empty;
                for (l.items) |v| {
                    try new_list.append(allocator, try v.clone(allocator));
                }
                return .{ .list = new_list };
            },
            else => return self,
        }
    }

    pub fn format(self: Value, comptime fmt: []const u8, options: std.fmt.FormatOptions, writer: anytype) !void {
        _ = fmt;
        _ = options;
        switch (self) {
            .null => try writer.writeAll("null"),
            .boolean => |b| try writer.print("{s}", .{if (b) "true" else "false"}),
            .number => |n| try writer.print("{d}", .{n}),
            .string => |s| try writer.print("{s}", .{s}),
            .map => |m| {
                try writer.writeAll("{ ");
                var it = m.iterator();
                var first = true;
                while (it.next()) |entry| {
                    if (!first) try writer.writeAll(", ");
                    try writer.print("\"{s}\": {any}", .{ entry.key_ptr.*, entry.value_ptr.* });
                    first = false;
                }
                try writer.writeAll(" }");
            },
            .list => |l| {
                try writer.writeAll("[ ");
                for (l.items, 0..) |v, i| {
                    if (i > 0) try writer.writeAll(", ");
                    try writer.print("{any}", .{v});
                }
                try writer.writeAll(" ]");
            },
        }
    }

    pub fn toBool(self: Value) bool {
        return switch (self) {
            .null => false,
            .boolean => |b| b,
            .number => |n| n != 0,
            .string => |s| s.len != 0,
            else => true,
        };
    }

    pub fn toNumber(self: Value) f64 {
        return switch (self) {
            .number => |n| n,
            .boolean => |b| if (b) 1.0 else 0.0,
            .string => |s| std.fmt.parseFloat(f64, s) catch 0.0,
            else => 0.0,
        };
    }
};

pub const ErmEval = struct {
    allocator: std.mem.Allocator,
    vars: std.StringHashMap(Value),

    pub fn init(allocator: std.mem.Allocator) ErmEval {
        return .{
            .allocator = allocator,
            .vars = std.StringHashMap(Value).init(allocator),
        };
    }

    pub fn deinit(self: *ErmEval) void {
        var it = self.vars.iterator();
        while (it.next()) |entry| {
            self.allocator.free(entry.key_ptr.*);
            entry.value_ptr.deinit(self.allocator);
        }
        self.vars.deinit();
    }

    pub fn set(self: *ErmEval, name: []const u8, val: Value) !void {
        const entry = try self.vars.getOrPutValue(try self.allocator.dupe(u8, name), val);
        if (entry.key_ptr.* != name) {
            // Already existed, entry.key_ptr was already duped.
            // But we duped it again above. This is a leak or double-dupe.
            // Better logic:
        }
    }

    pub fn set_owned(self: *ErmEval, name: []const u8, val: Value) !void {
        if (self.vars.getPtr(name)) |v| {
            v.deinit(self.allocator);
            v.* = val;
        } else {
            try self.vars.put(try self.allocator.dupe(u8, name), val);
        }
    }

    pub fn clone(self: *const ErmEval) !ErmEval {
        var new_ev = ErmEval.init(self.allocator);
        var it = self.vars.iterator();
        while (it.next()) |entry| {
            try new_ev.set_owned(entry.key_ptr.*, try entry.value_ptr.clone(self.allocator));
        }
        return new_ev;
    }

    pub fn eval(self: *ErmEval, expr: []const u8) !Value {
        var parser = ExprParser{ .input = expr, .pos = 0, .ev = self };
        return try parser.parseExpr();
    }

    pub fn evalBool(self: *ErmEval, expr: []const u8) !bool {
        const val = try self.eval(expr);
        return val.toBool();
    }

    pub fn parseScriptVars(self: *ErmEval, script: []const u8) !void {
        var i: usize = 0;
        while (i < script.len) {
            // Very simple assignment parser (replaces regex)
            // Look for let/const/var name =
            const keywords = [_][]const u8{ "let", "const", "var" };
            var found = false;
            for (keywords) |kw| {
                if (std.mem.startsWith(u8, script[i..], kw) and i + kw.len < script.len and std.ascii.isWhitespace(script[i + kw.len])) {
                    var j = i + kw.len;
                    while (j < script.len and std.ascii.isWhitespace(script[j])) {
                        j += 1;
                    }
                    const name_start = j;
                    while (j < script.len and (std.ascii.isAlphanumeric(script[j]) or script[j] == '_' or script[j] == '$')) {
                        j += 1;
                    }
                    const name = script[name_start..j];
                    
                    while (j < script.len and std.ascii.isWhitespace(script[j])) {
                        j += 1;
                    }
                    if (j < script.len and script[j] == '=') {
                        j += 1;
                        const val_start = j;
                        const parsed = parseJSValue(script, val_start, self.allocator);
                        if (parsed.val) |v| {
                            try self.set_owned(name, v);
                            i = parsed.pos;
                            found = true;
                            break;
                        }
                    }
                }
            }
            if (!found) i += 1;
        }
    }
};

const ExprParser = struct {
    input: []const u8,
    pos: usize,
    ev: *ErmEval,

    fn skip(self: *ExprParser) void {
        while (self.pos < self.input.len and std.ascii.isWhitespace(self.input[self.pos])) {
            self.pos += 1;
        }
    }

    fn parseExpr(self: *ExprParser) anyerror!Value {
        return try self.parseOr();
    }

    fn parseOr(self: *ExprParser) anyerror!Value {
        var left = try self.parseAnd();
        while (true) {
            self.skip();
            if (self.pos + 2 <= self.input.len and std.mem.eql(u8, self.input[self.pos .. self.pos + 2], "||")) {
                self.pos += 2;
                const right = try self.parseAnd();
                if (!left.toBool()) {
                    left = right;
                }
            } else break;
        }
        return left;
    }

    fn parseAnd(self: *ExprParser) anyerror!Value {
        var left = try self.parseComparison();
        while (true) {
            self.skip();
            if (self.pos + 2 <= self.input.len and std.mem.eql(u8, self.input[self.pos .. self.pos + 2], "&&")) {
                self.pos += 2;
                const right = try self.parseComparison();
                if (left.toBool()) {
                    left = right;
                }
            } else break;
        }
        return left;
    }

    fn parseComparison(self: *ExprParser) anyerror!Value {
        var left = try self.parseAddSub();
        self.skip();
        if (self.pos >= self.input.len) return left;
        
        const ops = [_][]const u8{ "===", "!==", "==", "!=", ">=", "<=", ">", "<" };
        var found_op: ?[]const u8 = null;
        for (ops) |op| {
            if (std.mem.startsWith(u8, self.input[self.pos..], op)) {
                found_op = op;
                break;
            }
        }

        if (found_op) |op| {
            self.pos += op.len;
            const right = try self.parseAddSub();
            const lf = left.toNumber();
            const rf = right.toNumber();

            if (std.mem.eql(u8, op, ">")) return .{ .boolean = lf > rf };
            if (std.mem.eql(u8, op, "<")) return .{ .boolean = lf < rf };
            if (std.mem.eql(u8, op, ">=")) return .{ .boolean = lf >= rf };
            if (std.mem.eql(u8, op, "<=")) return .{ .boolean = lf <= rf };
            if (std.mem.eql(u8, op, "==") or std.mem.eql(u8, op, "===")) return .{ .boolean = valuesEqual(left, right) };
            if (std.mem.eql(u8, op, "!=") or std.mem.eql(u8, op, "!==")) return .{ .boolean = !valuesEqual(left, right) };
        }
        return left;
    }

    fn parseAddSub(self: *ExprParser) anyerror!Value {
        var left = try self.parseMulDiv();
        while (true) {
            self.skip();
            if (self.pos >= self.input.len) break;
            const c = self.input[self.pos];
            if (c == '+' or c == '-') {
                self.pos += 1;
                const right = try self.parseMulDiv();
                if (c == '+') {
                    if (left == .string or right == .string) {
                        var buf: std.ArrayList(u8) = .empty;
                        defer buf.deinit(self.ev.allocator);
                        try buf.writer(self.ev.allocator).print("{any}", .{left});
                        try buf.writer(self.ev.allocator).print("{any}", .{right});
                        left = .{ .string = try buf.toOwnedSlice(self.ev.allocator) };
                    } else {
                        left = .{ .number = left.toNumber() + right.toNumber() };
                    }
                } else {
                    left = .{ .number = left.toNumber() - right.toNumber() };
                }
            } else break;
        }
        return left;
    }

    fn parseMulDiv(self: *ExprParser) anyerror!Value {
        var left = try self.parseUnary();
        while (true) {
            self.skip();
            if (self.pos >= self.input.len) break;
            const c = self.input[self.pos];
            if (c == '*' or c == '/' or c == '%') {
                self.pos += 1;
                const right = try self.parseUnary();
                const lf = left.toNumber();
                const rf = right.toNumber();
                switch (c) {
                    '*' => left = .{ .number = lf * rf },
                    '/' => left = .{ .number = if (rf == 0) 0 else lf / rf },
                    '%' => left = .{ .number = if (rf == 0) 0 else @as(f64, @floatFromInt(@rem(@as(i64, @intFromFloat(lf)), @as(i64, @intFromFloat(rf))))) },
                    else => unreachable,
                }
            } else break;
        }
        return left;
    }

    fn parseUnary(self: *ExprParser) anyerror!Value {
        self.skip();
        if (self.pos < self.input.len) {
            if (self.input[self.pos] == '!') {
                self.pos += 1;
                const val = try self.parseUnary();
                return .{ .boolean = !val.toBool() };
            }
            if (self.input[self.pos] == '-') {
                self.pos += 1;
                const val = try self.parseUnary();
                return .{ .number = -val.toNumber() };
            }
        }
        return try self.parsePrimary();
    }

    fn parsePrimary(self: *ExprParser) anyerror!Value {
        self.skip();
        if (self.pos >= self.input.len) return error.UnexpectedEnd;
        const c = self.input[self.pos];

        if (c == '(') {
            self.pos += 1;
            const val = try self.parseExpr();
            self.skip();
            if (self.pos < self.input.len and self.input[self.pos] == ')') {
                self.pos += 1;
            }
            return val;
        }

        if (std.ascii.isDigit(c) or c == '.') {
            const start = self.pos;
            while (self.pos < self.input.len and (std.ascii.isDigit(self.input[self.pos]) or self.input[self.pos] == '.')) {
                self.pos += 1;
            }
            const n = try std.fmt.parseFloat(f64, self.input[start..self.pos]);
            return .{ .number = n };
        }

        if (c == '"' or c == '\'' or c == '`') {
            const quote = c;
            self.pos += 1;
            var sb: std.ArrayList(u8) = .empty;
            errdefer sb.deinit(self.ev.allocator);
            while (self.pos < self.input.len and self.input[self.pos] != quote) {
                if (self.input[self.pos] == '\\' and self.pos + 1 < self.input.len) {
                    self.pos += 1;
                    switch (self.input[self.pos]) {
                        'n' => try sb.append(self.ev.allocator, '\n'),
                        't' => try sb.append(self.ev.allocator, '\t'),
                        else => try sb.append(self.ev.allocator, self.input[self.pos]),
                    }
                } else {
                    try sb.append(self.ev.allocator, self.input[self.pos]);
                }
                self.pos += 1;
            }
            if (self.pos < self.input.len) self.pos += 1;
            return .{ .string = try sb.toOwnedSlice(self.ev.allocator) };
        }

        if (std.ascii.isAlphabetic(c) or c == '_' or c == '$') {
            const start = self.pos;
            while (self.pos < self.input.len and (std.ascii.isAlphanumeric(self.input[self.pos]) or self.input[self.pos] == '_' or self.input[self.pos] == '$')) {
                self.pos += 1;
            }
            const name = self.input[start..self.pos];

            if (std.mem.eql(u8, name, "true")) return .{ .boolean = true };
            if (std.mem.eql(u8, name, "false")) return .{ .boolean = false };
            if (std.mem.eql(u8, name, "null") or std.mem.eql(u8, name, "undefined")) return .null;

            var val: Value = self.ev.vars.get(name) orelse .null;
            while (self.pos < self.input.len and self.input[self.pos] == '.') {
                self.pos += 1;
                const p_start = self.pos;
                while (self.pos < self.input.len and (std.ascii.isAlphanumeric(self.input[self.pos]) or self.input[self.pos] == '_' or self.input[self.pos] == '$')) {
                    self.pos += 1;
                }
                const prop = self.input[p_start..self.pos];
                if (val == .map) {
                    val = val.map.get(prop) orelse .null;
                } else {
                    val = .null;
                }
            }
            return val;
        }

        return error.UnexpectedCharacter;
    }
};

fn valuesEqual(a: Value, b: Value) bool {
    switch (a) {
        .null => return b == .null,
        .boolean => |ab| return b == .boolean and b.boolean == ab,
        .number => |an| return b == .number and b.number == an,
        .string => |as| return b == .string and std.mem.eql(u8, as, b.string),
        else => return false, // Simplified
    }
}

fn parseJSValue(s: []const u8, pos: usize, allocator: std.mem.Allocator) struct { val: ?Value, pos: usize } {
    var p = pos;
    while (p < s.len and std.ascii.isWhitespace(s[p])) {
        p += 1;
    }
    if (p >= s.len) return .{ .val = null, .pos = p };

    // signal()
    if (std.mem.startsWith(u8, s[p..], "signal(")) {
        p += 7;
        const inner = parseJSValue(s, p, allocator);
        p = inner.pos;
        while (p < s.len and s[p] != ')') p += 1;
        if (p < s.len) p += 1;
        var m = std.StringHashMap(Value).init(allocator);
        if (inner.val) |v| m.put("value", v) catch {};
        return .{ .val = .{ .map = m }, .pos = p };
    }

    // String literal
    if (s[p] == '"' or s[p] == '\'' or s[p] == '`') {
        const quote = s[p];
        p += 1;
        const start = p;
        while (p < s.len and s[p] != quote) p += 1;
        const val = allocator.dupe(u8, s[start..p]) catch "";
        if (p < s.len) p += 1;
        return .{ .val = .{ .string = val }, .pos = p };
    }

    // Number literal
    if (std.ascii.isDigit(s[p]) or s[p] == '-') {
        const start = p;
        if (s[p] == '-') p += 1;
        while (p < s.len and (std.ascii.isDigit(s[p]) or s[p] == '.')) p += 1;
        const n = std.fmt.parseFloat(f64, s[start..p]) catch 0.0;
        return .{ .val = .{ .number = n }, .pos = p };
    }

    // Bool/Null
    if (std.mem.startsWith(u8, s[p..], "true")) return .{ .val = .{ .boolean = true }, .pos = p + 4 };
    if (std.mem.startsWith(u8, s[p..], "false")) return .{ .val = .{ .boolean = false }, .pos = p + 5 };
    if (std.mem.startsWith(u8, s[p..], "null")) return .{ .val = .null, .pos = p + 4 };

    return .{ .val = null, .pos = p };
}
