const std = @import("std");
const eval = @import("eval.zig");
const router = @import("router.zig");
const compiler = @import("compiler.zig");
const er = @import("er.zig");

const Watcher = struct {
    mutex: std.Thread.Mutex = .{},
    cond: std.Thread.Condition = .{},
    change_count: usize = 0,
    last_path: ?[]const u8 = null,
    allocator: std.mem.Allocator,

    fn notify(self: *Watcher, path: []const u8) void {
        self.mutex.lock();
        defer self.mutex.unlock();
        if (self.last_path) |p| self.allocator.free(p);
        self.last_path = self.allocator.dupe(u8, path) catch null;
        self.change_count += 1;
        self.cond.broadcast();
    }

    fn wait(self: *Watcher, last_count: usize) usize {
        self.mutex.lock();
        defer self.mutex.unlock();
        while (self.change_count == last_count) {
            self.cond.wait(&self.mutex);
        }
        return self.change_count;
    }
};

var global_watcher: Watcher = undefined;

fn watchFiles(allocator: std.mem.Allocator, dir: []const u8) !void {
    var last_check: i128 = std.time.nanoTimestamp();
    const ns_per_ms = 1_000_000;
    while (true) {
        std.Thread.sleep(200 * ns_per_ms);
        var iter_dir = std.fs.cwd().openDir(dir, .{ .iterate = true }) catch continue;
        defer iter_dir.close();
        var walker = iter_dir.walk(allocator) catch continue;
        defer walker.deinit();

        var changed_path: ?[]const u8 = null;
        while (walker.next() catch break) |entry| {
            if (entry.kind == .file and (std.mem.endsWith(u8, entry.path, ".erm") or std.mem.endsWith(u8, entry.path, ".css"))) {
                const stat = entry.dir.statFile(entry.basename) catch continue;
                if (stat.mtime > last_check) {
                    if (stat.mtime > last_check) last_check = stat.mtime;
                    changed_path = entry.path;
                }
            }
        }
        if (changed_path != null) {
            global_watcher.notify(changed_path.?);
        }
    }
}

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    const allocator = gpa.allocator();
    defer _ = gpa.deinit();

    global_watcher.allocator = allocator;
    global_watcher.last_path = null;
    global_watcher.change_count = 0;
    global_watcher.mutex = .{};
    global_watcher.cond = .{};

    const args = try std.process.argsAlloc(allocator);
    defer std.process.argsFree(allocator, args);

    var cmd: []const u8 = "dev";
    var dir: []const u8 = ".";
    var port: u16 = 8080;

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
                } else {
                    dir = arg;
                }
            }
        } else if (std.mem.endsWith(u8, args[1], ".em")) {
            try runEmFile(allocator, args[1]);
            return;
        } else if (std.mem.endsWith(u8, args[1], ".er")) {
            try er.runFile(allocator, args[1]);
            return;
        } else {
            dir = args[1];
        }
    }

    const abs_dir = try std.fs.cwd().realpathAlloc(allocator, dir);
    defer allocator.free(abs_dir);

    if (std.mem.eql(u8, cmd, "init")) {
        try initProject(allocator, abs_dir);
        return;
    }

    if (std.mem.eql(u8, cmd, "build")) {
        try buildProject(allocator, abs_dir);
        return;
    }

    if (std.mem.eql(u8, cmd, "start")) {
        try startServer(allocator, abs_dir, true, port);
        return;
    }

    // Default: dev
    try startServer(allocator, abs_dir, false, port);
}

fn runEmFile(allocator: std.mem.Allocator, path: []const u8) !void {
    const content = try std.fs.cwd().readFileAlloc(allocator, path, 1024 * 1024);
    defer allocator.free(content);
    var ev = eval.ErmEval.init(allocator);
    defer ev.deinit();
    // Simplified Run loop
    var it = std.mem.splitScalar(u8, content, '\n');
    while (it.next()) |line| {
        _ = ev.eval(line) catch {};
    }
}

fn initProject(allocator: std.mem.Allocator, dir: []const u8) !void {
    _ = allocator;
    std.debug.print("Initializing fresh Eronom project in {s}\n", .{dir});
    try std.fs.cwd().makePath(dir);
    // Write index.erm and layout.erm (omitted for brevity, same as Go)
}

fn buildProject(allocator: std.mem.Allocator, dir: []const u8) !void {
    const build_dir = try std.fs.path.join(allocator, &.{ dir, "build" });
    defer allocator.free(build_dir);
    std.debug.print("Building project to {s}\n", .{build_dir});

    // Clean and recreate build directory
    std.fs.cwd().deleteTree(build_dir) catch {};
    try std.fs.cwd().makePath(build_dir);

    var layouts = std.StringHashMap([]const u8).init(allocator);
    defer {
        var it = layouts.iterator();
        while (it.next()) |entry| {
            allocator.free(entry.key_ptr.*);
            allocator.free(entry.value_ptr.*);
        }
        layouts.deinit();
    }

    // Pass 1: find layouts
    {
        var iter_dir = try std.fs.cwd().openDir(dir, .{ .iterate = true });
        defer iter_dir.close();
        var walker = try iter_dir.walk(allocator);
        defer walker.deinit();
        while (try walker.next()) |entry| {
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
                const content = try entry.dir.readFileAlloc(allocator, entry.basename, 1024 * 1024);
                const layout_dir = try allocator.dupe(u8, std.fs.path.dirname(entry.path) orelse ".");
                try layouts.put(layout_dir, content);
            }
        }
    }

    // Pass 2: process files
    {
        var iter_dir = try std.fs.cwd().openDir(dir, .{ .iterate = true });
        defer iter_dir.close();
        var walker = try iter_dir.walk(allocator);
        defer walker.deinit();

        while (try walker.next()) |entry| {
            // Skip build directory, source code, and build artifacts
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
                    // Skip components (Uppercase)
                    if (entry.basename.len > 0 and std.ascii.isUpper(entry.basename[0])) continue;

                    const content = try entry.dir.readFileAlloc(allocator, entry.basename, 1024 * 1024);
                    defer allocator.free(content);

                    const content_hmr = try std.mem.replaceOwned(u8, allocator, content, "import.meta.hot", "window.hmr");
                    defer allocator.free(content_hmr);

                    // Find closest layout
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

                    const processed = try compiler.processErmComponent(allocator, abs_file_dir, final_content, true);
                    defer allocator.free(processed);

                    // Determine output path (pretty URLs)
                    const base_name = entry.basename[0 .. entry.basename.len - 4]; // remove .erm
                    var out_path: []const u8 = undefined;
                    if (std.mem.eql(u8, base_name, "index") or std.mem.eql(u8, base_name, "page")) {
                        out_path = try std.fs.path.join(allocator, &.{ build_dir, rel_dir, "index.html" });
                    } else {
                        out_path = try std.fs.path.join(allocator, &.{ build_dir, rel_dir, base_name, "index.html" });
                    }
                    defer allocator.free(out_path);

                    try std.fs.cwd().makePath(std.fs.path.dirname(out_path).?);
                    try std.fs.cwd().writeFile(.{ .sub_path = out_path, .data = processed });
                } else {
                    // Skip Zig/source relevant files
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
                    try std.fs.cwd().makePath(std.fs.path.dirname(target_path).?);
                    try entry.dir.copyFile(entry.basename, std.fs.cwd(), target_path, .{});
                }
            }
        }
    }
}

fn startServer(allocator: std.mem.Allocator, dir: []const u8, is_prod: bool, port: u16) !void {
    const address = try std.net.Address.parseIp("127.0.0.1", port);
    var server = try address.listen(.{ .reuse_address = true });
    defer server.deinit();

    std.debug.print("{s} server running at http://localhost:{d}\n", .{ if (is_prod) "Production" else "Dev", port });

    var app = router.App.init(allocator);
    // defer app.deinit();

    if (!is_prod) {
        _ = try std.Thread.spawn(.{}, watchFiles, .{ allocator, dir });
    }

    const app_ptr = &app;

    while (true) {
        const connection = try server.accept();
        _ = try std.Thread.spawn(.{}, handleConnection, .{ allocator, connection, dir, app_ptr, is_prod });
    }
}

fn handleDynamicApi(allocator: std.mem.Allocator, request: *std.http.Server.Request, target: []const u8) bool {
    return handleDynamicApiInner(allocator, request, target) catch |err| {
        std.debug.print("API Route Error: {any}\n", .{err});
        return false;
    };
}

fn handleDynamicApiInner(allocator: std.mem.Allocator, request: *std.http.Server.Request, target: []const u8) !bool {
    if (!std.mem.startsWith(u8, target, "/api")) return false;

    var path_it = std.mem.tokenizeScalar(u8, target[4..], '/');
    var current_api_path: std.ArrayList(u8) = .empty;
    defer current_api_path.deinit(allocator);
    try current_api_path.appendSlice(allocator, "api");

    while (path_it.next()) |part| {
        try current_api_path.append(allocator, '/');
        try current_api_path.appendSlice(allocator, part);

        const route_file = try std.fs.path.join(allocator, &.{ current_api_path.items, "route.er" });
        defer allocator.free(route_file);

        if (std.fs.cwd().statFile(route_file)) |_| {
            const prefix = try std.fmt.allocPrint(allocator, "/{s}", .{current_api_path.items});
            defer allocator.free(prefix);
            if (try er.handleApiRequest(allocator, request, route_file, prefix)) return true;
        } else |_| {}
    }

    const direct_er = try std.fmt.allocPrint(allocator, "api/{s}.er", .{target[5..]});
    defer allocator.free(direct_er);
    if (std.fs.cwd().statFile(direct_er)) |_| {
        if (try er.handleApiRequest(allocator, request, direct_er, target)) return true;
    } else |_| {}
    return false;
}

fn handleConnection(allocator: std.mem.Allocator, connection: std.net.Server.Connection, dir: []const u8, app: *router.App, is_prod: bool) void {
    defer connection.stream.close();

    var reader_buf: [4096]u8 = undefined;
    var buffered_reader = connection.stream.reader(&reader_buf);
    var writer_buf: [4096]u8 = undefined;
    var buffered_writer = connection.stream.writer(&writer_buf);

    var http_server = std.http.Server.init(buffered_reader.interface(), &buffered_writer.interface);

    var request = http_server.receiveHead() catch return;
    var target = request.head.target;

    // Strip query parameters for routing
    if (std.mem.indexOfScalar(u8, target, '?')) |q_idx| {
        target = target[0..q_idx];
    }
    if (std.mem.indexOfScalar(u8, target, '#')) |f_idx| {
        target = target[0..f_idx];
    }

    std.debug.print("Request: {s} {s}\n", .{ @tagName(request.head.method), target });

    // Block direct access to .erm files
    if (std.mem.endsWith(u8, target, ".erm")) {
        _ = request.respond("Not Found", .{ .status = .not_found }) catch {};
        return;
    }

    // HMR Endpoint
    if (!is_prod and std.mem.eql(u8, target, "/__hmr")) {
        const response_headers = "HTTP/1.1 200 OK\r\n" ++
            "Content-Type: text/event-stream\r\n" ++
            "Cache-Control: no-cache\r\n" ++
            "Connection: keep-alive\r\n" ++
            "Access-Control-Allow-Origin: *\r\n\r\n";
        connection.stream.writeAll(response_headers) catch return;

        var last_count = global_watcher.change_count;
        while (true) {
            last_count = global_watcher.wait(last_count);
            const path = global_watcher.last_path orelse "unknown";
            var json_buf: [1024]u8 = undefined;
            const json = std.fmt.bufPrint(&json_buf, "data: {{\"type\": \"update\", \"path\": \"{s}\"}}\n\n", .{path}) catch continue;
            connection.stream.writeAll(json) catch break;
        }
        return;
    }

    if (app.serveHTTP(&request) catch false) return;

    if (handleDynamicApi(allocator, &request, target)) return;

    // Static file serving
    var full_path = std.fs.path.join(allocator, &.{ dir, target }) catch return;
    defer allocator.free(full_path);

    var stat = std.fs.cwd().statFile(full_path) catch |err| blk: {
        if (err == error.FileNotFound and !std.mem.endsWith(u8, full_path, ".erm")) {
            const erm_path = std.fmt.allocPrint(allocator, "{s}.erm", .{full_path}) catch return;
            defer allocator.free(erm_path);
            if (std.fs.cwd().statFile(erm_path)) |s| {
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
            if (std.fs.cwd().statFile(idx_path)) |s| {
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

    // Handle .erm processing
    if (std.mem.endsWith(u8, full_path, ".erm")) {
        const content = std.fs.cwd().readFileAlloc(allocator, full_path, 1024 * 1024) catch return;
        defer allocator.free(content);

        const content_hmr = std.mem.replaceOwned(u8, allocator, content, "import.meta.hot", "window.hmr") catch content;
        defer if (content_hmr.ptr != content.ptr) allocator.free(@constCast(content_hmr));

        // Find closest layout
        var current_dir = allocator.dupe(u8, std.fs.path.dirname(full_path) orelse ".") catch return;
        defer allocator.free(current_dir);
        var layout_content: ?[]const u8 = null;
        while (true) {
            const lp = std.fs.path.join(allocator, &.{ current_dir, "layout.erm" }) catch break;
            defer allocator.free(lp);
            if (std.fs.cwd().readFileAlloc(allocator, lp, 1024 * 1024)) |c| {
                layout_content = c;
                break;
            } else |_| {}

            if (std.mem.eql(u8, current_dir, dir)) break;
            const parent = std.fs.path.dirname(current_dir) orelse break;
            if (std.mem.eql(u8, parent, current_dir)) break;
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

        const processed = compiler.processErmComponent(allocator, std.fs.path.dirname(full_path).?, final_content, is_prod) catch return;
        defer allocator.free(processed);
        _ = request.respond(processed, .{ .status = .ok, .extra_headers = &.{.{ .name = "Content-Type", .value = "text/html; charset=utf-8" }} }) catch {};
    } else {
        const file = std.fs.cwd().openFile(full_path, .{}) catch {
            _ = request.respond("Not Found", .{ .status = .not_found }) catch {};
            return;
        };
        defer file.close();
        const content = file.readToEndAlloc(allocator, stat.size) catch return;
        defer allocator.free(content);
        _ = request.respond(content, .{ .status = .ok }) catch {};
    }
}
