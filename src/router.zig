const std = @import("std");

pub const H = std.StringHashMap([]const u8);

pub const Ctx = struct {
    allocator: std.mem.Allocator,
    request: *std.http.Server.Request,
    params: std.StringHashMap([]const u8),
    status: std.http.Status = .ok,

    pub fn param(self: *const Ctx, name: []const u8) []const u8 {
        return self.params.get(name) orelse "";
    }

    pub fn setStatus(self: *Ctx, status: std.http.Status) *Ctx {
        self.status = status;
        return self;
    }

    pub fn json(self: *Ctx, data: anytype) !void {
        var buf = std.ArrayList(u8).init(self.allocator);
        defer buf.deinit();
        try std.json.stringify(data, .{}, buf.writer());
        
        try self.request.respond(buf.items, .{
            .status = self.status,
            .extra_headers = &.{
                .{ .name = "Content-Type", .value = "application/json" },
                .{ .name = "Access-Control-Allow-Origin", .value = "*" },
            },
        });
    }

    pub fn sendString(self: *Ctx, text: []const u8) !void {
        try self.request.respond(text, .{
            .status = self.status,
            .extra_headers = &.{
                .{ .name = "Content-Type", .value = "text/plain; charset=utf-8" },
            },
        });
    }
};

pub const HandlerFunc = *const fn (c: *Ctx) anyerror!void;

pub const RouteEntry = struct {
    method: std.http.Method,
    path: []const u8,
    handler: HandlerFunc,
};

pub const App = struct {
    allocator: std.mem.Allocator,
    routes: std.ArrayList(RouteEntry),
    prefix: []const u8,

    pub fn init(allocator: std.mem.Allocator) App {
        return .{
            .allocator = allocator,
            .routes = .empty,
            .prefix = "",
        };
    }

    pub fn deinit(self: *App) void {
        for (self.routes.items) |r| {
            self.allocator.free(r.path);
        }
        self.routes.deinit(self.allocator);
    }

    pub fn handle(self: *App, method: std.http.Method, path: []const u8, h: HandlerFunc) !void {
        var full_path: std.ArrayList(u8) = .empty;
        defer full_path.deinit(self.allocator);
        try full_path.appendSlice(self.allocator, self.prefix);
        if (!std.mem.startsWith(u8, path, "/")) try full_path.append(self.allocator, '/');
        try full_path.appendSlice(self.allocator, path);
        
        // Trim trailing slash if not root
        if (full_path.items.len > 1 and full_path.items[full_path.items.len - 1] == '/') {
            _ = full_path.pop();
        }

        try self.routes.append(self.allocator, .{
            .method = method,
            .path = try full_path.toOwnedSlice(self.allocator),
            .handler = h,
        });
    }

    pub fn get(self: *App, path: []const u8, h: HandlerFunc) !void { try self.handle(.GET, path, h); }
    pub fn post(self: *App, path: []const u8, h: HandlerFunc) !void { try self.handle(.POST, path, h); }
    pub fn put(self: *App, path: []const u8, h: HandlerFunc) !void { try self.handle(.PUT, path, h); }
    pub fn delete(self: *App, path: []const u8, h: HandlerFunc) !void { try self.handle(.DELETE, path, h); }

    pub fn serveHTTP(self: *App, req: *std.http.Server.Request) !bool {
        const path = req.head.target;
        // Trim /api prefix
        var clean_path = path;
        if (std.mem.startsWith(u8, clean_path, "/api")) {
            clean_path = clean_path[4..];
        }
        if (clean_path.len == 0) clean_path = "/";

        for (self.routes.items) |r| {
            if (r.method == req.head.method) {
                if (try matchPath(self.allocator, r.path, clean_path)) |params| {
                    var ctx = Ctx{
                        .allocator = self.allocator,
                        .request = req,
                        .params = params,
                    };
                    defer {
                        var it = ctx.params.iterator();
                        while (it.next()) |entry| {
                            self.allocator.free(entry.key_ptr.*);
                            self.allocator.free(entry.value_ptr.*);
                        }
                        ctx.params.deinit();
                    }
                    try r.handler(&ctx);
                    return true;
                }
            }
        }
        return false;
    }
};

fn matchPath(allocator: std.mem.Allocator, pattern: []const u8, path: []const u8) !?std.StringHashMap([]const u8) {
    if (std.mem.eql(u8, pattern, "/") and std.mem.eql(u8, path, "/")) {
        return std.StringHashMap([]const u8).init(allocator);
    }

    var pat_it = std.mem.tokenizeScalar(u8, pattern, '/');
    var path_it = std.mem.tokenizeScalar(u8, path, '/');

    var params = std.StringHashMap([]const u8).init(allocator);
    errdefer {
        var it = params.iterator();
        while (it.next()) |entry| {
            allocator.free(entry.key_ptr.*);
            allocator.free(entry.value_ptr.*);
        }
        params.deinit();
    }

    while (true) {
        const pat_part = pat_it.next();
        const path_part = path_it.next();

        if (pat_part == null and path_part == null) return params;
        if (pat_part == null or path_part == null) break;

        if (std.mem.startsWith(u8, pat_part.?, ":")) {
            try params.put(try allocator.dupe(u8, pat_part.?[1..]), try allocator.dupe(u8, path_part.?));
        } else if (!std.mem.eql(u8, pat_part.?, path_part.?)) {
            break;
        }
    }

    // Clean up if no match
    var it = params.iterator();
    while (it.next()) |entry| {
        allocator.free(entry.key_ptr.*);
        allocator.free(entry.value_ptr.*);
    }
    params.deinit();
    return null;
}
