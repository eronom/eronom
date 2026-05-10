const std = @import("std");
const eval = @import("eval.zig");
const router = @import("router.zig");
const compiler = @import("compiler.zig");
const er = @import("er.zig");

const Watcher = struct {
    mutex: std.Io.Mutex = .init,
    cond: std.Io.Condition = .init,
    change_count: usize = 0,
    last_path: ?[]const u8 = null,
    allocator: std.mem.Allocator = undefined,

    pub fn notify(self: *Watcher, io: std.Io, path: []const u8) void {
        self.mutex.lockUncancelable(io);
        defer self.mutex.unlock(io);
        if (self.last_path) |old| self.allocator.free(old);
        self.last_path = self.allocator.dupe(u8, path) catch null;
        self.change_count += 1;
        self.cond.broadcast(io);
    }

    pub fn wait(self: *Watcher, io: std.Io, last_count: usize) usize {
        self.mutex.lockUncancelable(io);
        defer self.mutex.unlock(io);
        while (self.change_count == last_count) {
            self.cond.waitUncancelable(io, &self.mutex);
        }
        return self.change_count;
    }
};

var global_watcher: Watcher = undefined;

fn watchFiles(allocator: std.mem.Allocator, io: std.Io, dir: []const u8) !void {
    var last_check = std.Io.Timestamp.now(io, .awake);
    while (true) {
        io.sleep(std.Io.Duration.fromMilliseconds(200), .awake) catch {};
        const changed = try checkDirChanged(allocator, io, dir, &last_check);
        if (changed) |path| {
            global_watcher.notify(io, path);
        }
    }
}

fn checkDirChanged(allocator: std.mem.Allocator, io: std.Io, dir: []const u8, last_check: *std.Io.Timestamp) !?[]const u8 {
    var d = try std.Io.Dir.cwd().openDir(io, dir, .{ .iterate = true });
    defer d.close(io);

    var it = d.iterate();
    while (try it.next(io)) |entry| {
        if (entry.kind == .file) {
            const stat = try d.statFile(io, entry.name, .{});
            const mtime = stat.mtime;
            if (mtime.nanoseconds > last_check.nanoseconds) {
                last_check.* = mtime;
                return try allocator.dupe(u8, entry.name);
            }
        } else if (entry.kind == .directory) {
            if (std.mem.eql(u8, entry.name, ".zig-cache")) continue;
            const sub_path = try std.fs.path.join(allocator, &.{ dir, entry.name });
            defer allocator.free(sub_path);
            if (try checkDirChanged(allocator, io, sub_path, last_check)) |path| {
                return path;
            }
        }
    }
    return null;
}

pub fn main(init: std.process.Init) !void {
    const allocator = init.gpa;
    const io = init.io;

    global_watcher.allocator = allocator;
    global_watcher.last_path = null;
    global_watcher.mutex = .init;
    global_watcher.cond = .init;

    const args = try init.minimal.args.toSlice(init.arena.allocator());

    var cmd: []const u8 = "dev";
    var dir: []const u8 = ".";
    var port: u16 = 8080;
    var port_from_cli = false;

    if (args.len > 1) {
        if (std.mem.eql(u8, args[1], "build") or std.mem.eql(u8, args[1], "dev") or std.mem.eql(u8, args[1], "start") or std.mem.eql(u8, args[1], "init")) {
            cmd = args[1];
            var arg_idx: usize = 2;
            while (arg_idx < args.len) : (arg_idx += 1) {
                const arg = args[arg_idx];
                if (std.mem.eql(u8, arg, "for") or std.mem.eql(u8, arg, "on") or std.mem.eql(u8, arg, "port")) {
                    continue;
                }
                const maybe_port = std.fmt.parseInt(u16, arg, 10) catch null;
                if (maybe_port) |p| {
                    port = p;
                    port_from_cli = true;
                } else {
                    dir = arg;
                }
            }
        } else if (std.mem.endsWith(u8, args[1], ".em")) {
            try runEmFile(allocator, io, args[1]);
            return;
        } else if (std.mem.endsWith(u8, args[1], ".er")) {
            try er.runFile(allocator, io, args[1]);
            return;
        } else {
            dir = args[1];
        }
    }

    if (args.len > 1 and (std.mem.eql(u8, args[1], "help") or std.mem.eql(u8, args[1], "--help") or std.mem.eql(u8, args[1], "-h"))) {
        std.debug.print("Usage: eronom [command] [dir] [port]\n", .{});
        std.debug.print("Commands:\n", .{});
        std.debug.print("  dev     Start development server (default)\n", .{});
        std.debug.print("  build   Build project for production\n", .{});
        std.debug.print("  start   Start production server\n", .{});
        std.debug.print("  init    Initialize fresh project\n", .{});
        std.debug.print("  [file]  Run an .er or .em file directly\n", .{});
        return;
    }

    const abs_dir = try std.Io.Dir.cwd().realPathFileAlloc(io, dir, allocator);
    defer allocator.free(abs_dir);

    if (!port_from_cli) {
        var config_vars = std.StringHashMap([]const u8).init(allocator);
        var allocated_keys: std.ArrayList([]const u8) = .empty;
        var routes: std.ArrayList(er.Route) = .empty;
        defer {
            var it = config_vars.valueIterator();
            while (it.next()) |v| allocator.free(v.*);
            for (allocated_keys.items) |k| allocator.free(k);
            config_vars.deinit();
            allocated_keys.deinit(allocator);
            for (routes.items) |r| allocator.free(r.path);
            routes.deinit(allocator);
        }

        const config_files = [_][]const u8{ "config.er", "config.erm" };
        for (config_files) |cf| {
            const cp = std.fs.path.join(allocator, &.{ abs_dir, cf }) catch continue;
            defer allocator.free(cp);
            if (std.Io.Dir.cwd().statFile(io, cp, .{})) |_| {
                er.evaluateFile(allocator, io, cp, &config_vars, &allocated_keys, &routes) catch continue;
                if (config_vars.get("config.server.port")) |ps| {
                    port = std.fmt.parseInt(u16, ps, 10) catch port;
                }
                break;
            } else |_| {}
        }
    }

    if (std.mem.eql(u8, cmd, "init")) {
        try initProject(allocator, io, abs_dir);
        return;
    }

    if (std.mem.eql(u8, cmd, "build")) {
        try buildProject(allocator, io, abs_dir);
        return;
    }

    if (std.mem.eql(u8, cmd, "start")) {
        try startServer(allocator, io, abs_dir, true, port);
        return;
    }

    try startServer(allocator, io, abs_dir, false, port);
}

fn runEmFile(allocator: std.mem.Allocator, io: std.Io, path: []const u8) !void {
    try er.runFile(allocator, io, path);
}

fn initProject(allocator: std.mem.Allocator, io: std.Io, dir: []const u8) !void {
    _ = allocator;
    std.debug.print("Initializing fresh Eronom project in {s}\n", .{dir});
    try std.Io.Dir.cwd().createDirPath(io, dir);
}

fn buildProject(allocator: std.mem.Allocator, io: std.Io, dir: []const u8) !void {
    const build_dir = try std.fs.path.join(allocator, &.{ dir, "build" });
    defer allocator.free(build_dir);
    std.debug.print("Building project to {s}\n", .{build_dir});

    std.Io.Dir.cwd().deleteTree(io, build_dir) catch {};
    try std.Io.Dir.cwd().createDirPath(io, build_dir);

    var layouts = std.StringHashMap([]const u8).init(allocator);
    defer {
        var it = layouts.iterator();
        while (it.next()) |entry| {
            allocator.free(entry.key_ptr.*);
            allocator.free(entry.value_ptr.*);
        }
        layouts.deinit();
    }

    {
        var iter_dir = try std.Io.Dir.cwd().openDir(io, dir, .{ .iterate = true });
        defer iter_dir.close(io);
        var walker = try iter_dir.walk(allocator);
        defer walker.deinit();
        while (try walker.next(io)) |entry| {
            const skip_dirs = [_][]const u8{ "build", "src", "zig-out", ".zig-cache", "test", ".git", ".github", "tmp", ".agents" };
            var skip = false;
            for (skip_dirs) |sd| {
                if (std.mem.startsWith(u8, entry.path, sd)) {
                    skip = true;
                    break;
                }
            }
            if (skip) continue;
            if (std.mem.eql(u8, entry.basename, "layout.erm")) {
                const content = try entry.dir.readFileAlloc(io, entry.basename, allocator, @enumFromInt(1024 * 1024));
                const layout_dir = try allocator.dupe(u8, std.fs.path.dirname(entry.path) orelse ".");
                try layouts.put(layout_dir, content);
            }
        }
    }

    {
        var iter_dir = try std.Io.Dir.cwd().openDir(io, dir, .{ .iterate = true });
        defer iter_dir.close(io);
        var walker = try iter_dir.walk(allocator);
        defer walker.deinit();

        while (try walker.next(io)) |entry| {
            const skip_dirs = [_][]const u8{ "build", "src", "zig-out", ".zig-cache", "test", ".git", ".github", "tmp", ".agents" };
            var skip = false;
            for (skip_dirs) |sd| {
                if (std.mem.startsWith(u8, entry.path, sd)) {
                    skip = true;
                    break;
                }
            }
            if (skip) continue;
            if (std.mem.startsWith(u8, entry.basename, ".")) continue;

            const rel_dir = std.fs.path.dirname(entry.path) orelse ".";

            if (entry.kind == .file) {
                if (std.mem.endsWith(u8, entry.basename, ".erm")) {
                    if (std.mem.eql(u8, entry.basename, "layout.erm") or std.mem.eql(u8, entry.basename, "virtual_root.erm")) continue;
                    if (entry.basename.len > 0 and std.ascii.isUpper(entry.basename[0])) continue;

                    const content = try entry.dir.readFileAlloc(io, entry.basename, allocator, @enumFromInt(1024 * 1024));
                    defer allocator.free(content);

                    const content_hmr = try std.mem.replaceOwned(u8, allocator, content, "import.meta.hot", "window.hmr");
                    defer allocator.free(content_hmr);

                    var current_dir = try allocator.dupe(u8, rel_dir);
                    defer allocator.free(current_dir);
                    var layout_content: ?[]const u8 = null;
                    while (true) {
                        if (layouts.get(current_dir)) |l| {
                            layout_content = l;
                            break;
                        }
                        if (std.mem.eql(u8, current_dir, ".") or std.mem.eql(u8, current_dir, "")) break;
                        const parent = std.fs.path.dirname(current_dir) orelse ".";
                        const next_dir = try allocator.dupe(u8, parent);
                        allocator.free(current_dir);
                        current_dir = next_dir;
                    }

                    var final_content: []const u8 = undefined;
                    if (layout_content) |l| {
                        const replaced = try std.mem.replaceOwned(u8, allocator, l, "<slot />", content_hmr);
                        const replaced2 = try std.mem.replaceOwned(u8, allocator, replaced, "<slot></slot>", content_hmr);
                        allocator.free(replaced);
                        final_content = replaced2;
                    } else {
                        final_content = try allocator.dupe(u8, content_hmr);
                    }
                    defer allocator.free(final_content);

                    const file_dir = std.fs.path.dirname(entry.path) orelse ".";
                    const abs_file_dir = try std.fs.path.join(allocator, &.{ dir, file_dir });
                    defer allocator.free(abs_file_dir);

                    const processed = try compiler.processErmComponent(allocator, io, abs_file_dir, final_content, true);
                    defer allocator.free(processed);

                    const base_name = entry.basename[0 .. entry.basename.len - 4];
                    var out_path: []const u8 = undefined;
                    if (std.mem.eql(u8, base_name, "index") or std.mem.eql(u8, base_name, "page")) {
                        out_path = try std.fs.path.join(allocator, &.{ build_dir, rel_dir, "index.html" });
                    } else {
                        out_path = try std.fs.path.join(allocator, &.{ build_dir, rel_dir, base_name, "index.html" });
                    }
                    defer allocator.free(out_path);

                    try std.Io.Dir.cwd().createDirPath(io, std.fs.path.dirname(out_path).?);
                    try std.Io.Dir.cwd().writeFile(io, .{ .sub_path = out_path, .data = processed });
                } else {
                    const skip_exts = [_][]const u8{ ".zig", ".go", ".mod", ".sum" };
                    var skip_file = false;
                    for (skip_exts) |ext| {
                        if (std.mem.endsWith(u8, entry.basename, ext)) {
                            skip_file = true;
                            break;
                        }
                    }
                    if (std.mem.eql(u8, entry.basename, "eronom")) skip_file = true;
                    if (skip_file) continue;

                    const target_path = try std.fs.path.join(allocator, &.{ build_dir, entry.path });
                    defer allocator.free(target_path);
                    try std.Io.Dir.cwd().createDirPath(io, std.fs.path.dirname(target_path).?);
                    try entry.dir.copyFile(entry.basename, std.Io.Dir.cwd(), target_path, io, .{});
                }
            }
        }
    }
}

fn startServer(allocator: std.mem.Allocator, io: std.Io, dir: []const u8, is_prod: bool, initial_port: u16) !void {
    var port = initial_port;
    var server: std.Io.net.Server = undefined;
    while (true) {
        const address = try std.Io.net.IpAddress.parse("0.0.0.0", port);
        server = address.listen(io, .{ .reuse_address = false }) catch |err| {
            if (err == error.AddressInUse) {
                std.debug.print("Port {d} already opened, trying {d}...\n", .{ port, port + 1 });
                port += 1;
                continue;
            }
            return err;
        };
        break;
    }
    defer server.deinit(io);

    std.debug.print("{s} server running at http://localhost:{d}\n", .{ if (is_prod) "Production" else "Dev", port });

    var app = router.App.init(allocator);

    if (!is_prod) {
        _ = try std.Thread.spawn(.{}, watchFiles, .{ allocator, io, dir });
    }

    const app_ptr = &app;

    while (true) {
        const connection = try server.accept(io);
        _ = try std.Thread.spawn(.{}, handleConnection, .{ allocator, io, connection, dir, app_ptr, is_prod });
    }
}

fn handleDynamicApi(allocator: std.mem.Allocator, io: std.Io, request: *std.http.Server.Request, target: []const u8) bool {
    return handleDynamicApiInner(allocator, io, request, target) catch |err| {
        std.debug.print("API Route Error: {any}\n", .{err});
        return false;
    };
}

fn handleDynamicApiInner(allocator: std.mem.Allocator, io: std.Io, request: *std.http.Server.Request, target: []const u8) !bool {
    if (!std.mem.startsWith(u8, target, "/api/")) return false;

    var path_it = std.mem.tokenizeScalar(u8, target[4..], '/');
    var current_api_path: std.ArrayList(u8) = .empty;
    defer current_api_path.deinit(allocator);
    try current_api_path.appendSlice(allocator, "api");

    while (path_it.next()) |part| {
        try current_api_path.append(allocator, '/');
        try current_api_path.appendSlice(allocator, part);

        const route_file = try std.fs.path.join(allocator, &.{ current_api_path.items, "route.er" });
        defer allocator.free(route_file);

        if (std.Io.Dir.cwd().statFile(io, route_file, .{})) |_| {
            const prefix = try std.fmt.allocPrint(allocator, "/{s}", .{current_api_path.items});
            defer allocator.free(prefix);
            if (try er.handleApiRequest(allocator, io, request, route_file, prefix)) return true;
        } else |_| {}
    }

    const direct_er = try std.fmt.allocPrint(allocator, "api/{s}.er", .{target[5..]});
    defer allocator.free(direct_er);
    if (std.Io.Dir.cwd().statFile(io, direct_er, .{})) |_| {
        if (try er.handleApiRequest(allocator, io, request, direct_er, target)) return true;
    } else |_| {}
    return false;
}

fn handleConnection(allocator: std.mem.Allocator, io: std.Io, connection: std.Io.net.Stream, dir: []const u8, app: *router.App, is_prod: bool) !void {
    defer connection.close(io);

    var reader_buf: [4096]u8 = undefined;
    var reader = connection.reader(io, &reader_buf);
    var writer_buf: [4096]u8 = undefined;
    var writer = connection.writer(io, &writer_buf);

    var http_server = std.http.Server.init(&reader.interface, &writer.interface);
    var request = http_server.receiveHead() catch return;
    var target = request.head.target;

    if (std.mem.indexOfScalar(u8, target, '?')) |q_idx| target = target[0..q_idx];
    if (std.mem.indexOfScalar(u8, target, '#')) |f_idx| target = target[0..f_idx];

    std.debug.print("Request: {s} {s}\n", .{ @tagName(request.head.method), target });

    if (std.mem.endsWith(u8, target, ".erm")) {
        _ = request.respond("Not Found", .{ .status = .not_found }) catch {};
        return;
    }

    if (!is_prod and std.mem.eql(u8, target, "/__hmr")) {
        const response_headers = "HTTP/1.1 200 OK\r\n" ++
            "Content-Type: text/event-stream\r\n" ++
            "Cache-Control: no-cache\r\n" ++
            "Connection: keep-alive\r\n" ++
            "Access-Control-Allow-Origin: *\r\n\r\n";
        {
            var hmr_buf: [1024]u8 = undefined;
            var hmr_writer = connection.writer(io, &hmr_buf);
            hmr_writer.interface.writeAll(response_headers) catch return;
            hmr_writer.interface.flush() catch return;
        }

        var last_count = global_watcher.change_count;
        while (true) {
            last_count = global_watcher.wait(io, last_count);
            const path = global_watcher.last_path orelse "unknown";
            var json_buf: [1024]u8 = undefined;
            const json = std.fmt.bufPrint(&json_buf, "data: {{\"type\": \"update\", \"path\": \"{s}\"}}\n\n", .{path}) catch continue;

            var hmr_buf: [1024]u8 = undefined;
            var hmr_writer = connection.writer(io, &hmr_buf);
            hmr_writer.interface.writeAll(json) catch break;
            hmr_writer.interface.flush() catch break;
        }
        return;
    }

    if (app.serveHTTP(&request) catch false) return;
    if (handleDynamicApi(allocator, io, &request, target)) return;

    {
        const server_er = try std.fs.path.join(allocator, &.{ dir, "server.er" });
        defer allocator.free(server_er);
        if (std.Io.Dir.cwd().statFile(io, server_er, .{})) |_| {
            if (er.handleApiRequest(allocator, io, &request, server_er, "") catch |err| blk: {
                std.debug.print("Error in server.er: {any}\n", .{err});
                break :blk false;
            }) return;
        } else |_| {}
    }

    var full_path = std.fs.path.join(allocator, &.{ dir, target }) catch return;
    defer allocator.free(full_path);

    var stat = std.Io.Dir.cwd().statFile(io, full_path, .{} ) catch |err| blk: {
        if (err == error.FileNotFound and !std.mem.endsWith(u8, full_path, ".erm")) {
            const erm_path = try std.fmt.allocPrint(allocator, "{s}.erm", .{full_path});
            defer allocator.free(erm_path);
            if (std.Io.Dir.cwd().statFile(io, erm_path, .{})) |s| {
                const new_path = allocator.dupe(u8, erm_path) catch return;
                allocator.free(full_path);
                full_path = new_path;
                break :blk s;
            } else |_| {}
        }
        _ = request.respond("Not Found", .{ .status = .not_found }) catch {};
        return;
    };

    if (stat.kind == .directory) {
        var found = false;
        const index_files = [_][]const u8{ "index.erm", "page.erm", "index.html" };
        for (index_files) |idx_file| {
            const idx_path = std.fs.path.join(allocator, &.{ full_path, idx_file }) catch continue;
            defer allocator.free(idx_path);
            if (std.Io.Dir.cwd().statFile(io, idx_path, .{})) |s| {
                stat = s;
                const new_path = allocator.dupe(u8, idx_path) catch continue;
                allocator.free(full_path);
                full_path = new_path;
                found = true;
                break;
            } else |_| {}
        }
        if (!found) {
            _ = request.respond("Not Found", .{ .status = .not_found }) catch {};
            return;
        }
    }

    if (std.mem.endsWith(u8, full_path, ".erm")) {
        const content = std.Io.Dir.cwd().readFileAlloc(io, full_path, allocator, @enumFromInt(1024 * 1024)) catch return;
        defer allocator.free(content);
        const content_hmr = std.mem.replaceOwned(u8, allocator, content, "import.meta.hot", "window.hmr") catch content;
        defer if (content_hmr.ptr != content.ptr) allocator.free(@constCast(content_hmr));

        var current_dir = allocator.dupe(u8, std.fs.path.dirname(full_path) orelse ".") catch return;
        defer allocator.free(current_dir);
        var layout_content: ?[]const u8 = null;
        while (true) {
            const lp = std.fs.path.join(allocator, &.{ current_dir, "layout.erm" }) catch break;
            defer allocator.free(lp);
            if (std.Io.Dir.cwd().readFileAlloc(io, lp, allocator, @enumFromInt(1024 * 1024))) |c| {
                layout_content = c;
                break;
            } else |_| {}
            if (std.mem.eql(u8, current_dir, dir)) break;
            const parent = std.fs.path.dirname(current_dir) orelse break;
            const next = allocator.dupe(u8, parent) catch break;
            allocator.free(current_dir);
            current_dir = next;
        }

        var final_content: []const u8 = undefined;
        var owned_final = false;
        if (layout_content) |l| {
            defer allocator.free(l);
            const replaced = std.mem.replaceOwned(u8, allocator, l, "<slot />", content_hmr) catch l;
            const replaced2 = std.mem.replaceOwned(u8, allocator, replaced, "<slot></slot>", content_hmr) catch replaced;
            if (replaced.ptr != l.ptr and replaced.ptr != replaced2.ptr) allocator.free(replaced);
            final_content = replaced2;
            owned_final = (replaced2.ptr != l.ptr);
        } else {
            final_content = content_hmr;
            owned_final = false;
        }
        defer if (owned_final) allocator.free(@constCast(final_content));

        const processed = try compiler.processErmComponent(allocator, io, std.fs.path.dirname(full_path).?, final_content, is_prod);
        defer allocator.free(processed);
        _ = request.respond(processed, .{ .status = .ok, .extra_headers = &.{.{ .name = "Content-Type", .value = "text/html; charset=utf-8" }} }) catch {};
    } else {
        const content = try std.Io.Dir.cwd().readFileAlloc(io, full_path, allocator, @enumFromInt(stat.size));
        defer allocator.free(content);
        _ = request.respond(content, .{ .status = .ok }) catch {};
    }
}
