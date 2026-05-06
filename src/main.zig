const std = @import("std");
const eval = @import("eval.zig");
const router = @import("router.zig");
const compiler = @import("compiler.zig");

const Watcher = struct {
    mutex: std.Thread.Mutex = .{},
    cond: std.Thread.Condition = .{},
    change_count: usize = 0,

    fn notify(self: *Watcher) void {
        self.mutex.lock();
        defer self.mutex.unlock();
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

var global_watcher = Watcher{};

fn watchFiles(allocator: std.mem.Allocator, dir: []const u8) !void {
    var last_check: i128 = std.time.nanoTimestamp();
    const ns_per_ms = 1_000_000;
    while (true) {
        std.Thread.sleep(200 * ns_per_ms);
        var iter_dir = std.fs.cwd().openDir(dir, .{ .iterate = true }) catch continue;
        defer iter_dir.close();
        var walker = iter_dir.walk(allocator) catch continue;
        defer walker.deinit();

        var changed = false;
        while (walker.next() catch break) |entry| {
            if (entry.kind == .file and (std.mem.endsWith(u8, entry.path, ".erm") or std.mem.endsWith(u8, entry.path, ".css"))) {
                const stat = entry.dir.statFile(entry.basename) catch continue;
                if (stat.mtime > last_check) {
                    changed = true;
                    if (stat.mtime > last_check) last_check = stat.mtime;
                }
            }
        }
        if (changed) {
            global_watcher.notify();
        }
    }
}

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    const allocator = gpa.allocator();
    defer _ = gpa.deinit();

    const args = try std.process.argsAlloc(allocator);
    defer std.process.argsFree(allocator, args);

    var cmd: []const u8 = "dev";
    var dir: []const u8 = ".";

    if (args.len > 1) {
        if (std.mem.eql(u8, args[1], "build") or std.mem.eql(u8, args[1], "dev") or std.mem.eql(u8, args[1], "start") or std.mem.eql(u8, args[1], "init")) {
            cmd = args[1];
            if (args.len > 2) dir = args[2];
        } else if (std.mem.endsWith(u8, args[1], ".em")) {
            try runEmFile(allocator, args[1]);
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
        try startServer(allocator, abs_dir, true);
        return;
    }

    // Default: dev
    try startServer(allocator, abs_dir, false);
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
    // Recursive walk and process (omitted for brevity)
}

fn startServer(allocator: std.mem.Allocator, dir: []const u8, is_prod: bool) !void {
    const port: u16 = 8080;
    const address = try std.net.Address.parseIp("127.0.0.1", port);
    var server = try address.listen(.{ .reuse_address = true });
    defer server.deinit();

    std.debug.print("{s} server running at http://localhost:{d}\n", .{ if (is_prod) "Production" else "Dev", port });

    var app = router.App.init(allocator);
    // defer app.deinit();

    if (!is_prod) {
        _ = try std.Thread.spawn(.{}, watchFiles, .{ allocator, dir });
    }

    while (true) {
        const connection = try server.accept();
        _ = try std.Thread.spawn(.{}, handleConnection, .{ allocator, connection, dir, &app });
    }
}

fn handleConnection(allocator: std.mem.Allocator, connection: std.net.Server.Connection, dir: []const u8, app: *router.App) void {
    defer connection.stream.close();

    var reader_buf: [4096]u8 = undefined;
    var buffered_reader = connection.stream.reader(&reader_buf);
    var writer_buf: [4096]u8 = undefined;
    var buffered_writer = connection.stream.writer(&writer_buf);
    
    var http_server = std.http.Server.init(buffered_reader.interface(), &buffered_writer.interface);

    var request = http_server.receiveHead() catch return;
    
    const target = request.head.target;

    // HMR Endpoint
    if (std.mem.eql(u8, target, "/__hmr")) {
        const response_headers = "HTTP/1.1 200 OK\r\n" ++
                               "Content-Type: text/event-stream\r\n" ++
                               "Cache-Control: no-cache\r\n" ++
                               "Connection: keep-alive\r\n" ++
                               "Access-Control-Allow-Origin: *\r\n\r\n";
        connection.stream.writeAll(response_headers) catch return;

        var last_count = global_watcher.change_count;
        while (true) {
            last_count = global_watcher.wait(last_count);
            connection.stream.writeAll("data: {\"type\": \"update\"}\n\n") catch break;
        }
        return;
    }

    if (app.serveHTTP(&request) catch false) return;

    // Static file serving
    var full_path = std.fs.path.join(allocator, &.{ dir, target }) catch return;
    defer allocator.free(full_path);

    var stat = std.fs.cwd().statFile(full_path) catch {
        _ = request.respond("Not Found", .{ .status = .not_found }) catch {};
        return;
    };

    if (stat.kind == .directory) {
        var found = false;
        const index_files = [_][]const u8{ "index.erm", "index.html" };
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
        const processed = compiler.processErmComponent(allocator, std.fs.path.dirname(full_path).?, content) catch return;
        defer allocator.free(processed);
        _ = request.respond(processed, .{ .status = .ok, .extra_headers = &.{.{ .name = "Content-Type", .value = "text/html" }} }) catch {};
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
