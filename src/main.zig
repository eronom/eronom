const std = @import("std");
const eval = @import("eval.zig");
const router = @import("router.zig");
const compiler = @import("compiler.zig");

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
    defer app.deinit();

    // Register routes...
    
    while (true) {
        const connection = try server.accept();
        defer connection.stream.close();

        var read_buffer: [4096]u8 = undefined;
        var reader_buf: [1024]u8 = undefined;
        var reader = connection.stream.reader(&reader_buf);
        var writer_buf: [1024]u8 = undefined;
        var writer = connection.stream.writer(&writer_buf);
        var http_server = std.http.Server.init(&reader, &writer);

        var request = try http_server.receive(&read_buffer);
        
        if (try app.serveHTTP(&request)) continue;

        // Static file serving
        const target = request.head.target;
        const full_path = try std.fs.path.join(allocator, &.{ dir, target });
        defer allocator.free(full_path);
        
        // Handle .erm processing
        if (std.mem.endsWith(u8, full_path, ".erm")) {
            const content = try std.fs.cwd().readFileAlloc(allocator, full_path, 1024 * 1024);
            defer allocator.free(content);
            const processed = try compiler.processErmComponent(allocator, std.fs.path.dirname(full_path).?, content);
            defer allocator.free(processed);
            try request.respond(processed, .{ .status = .ok, .extra_headers = &.{ .{ .name = "Content-Type", .value = "text/html" } } });
        } else {
            const file = std.fs.cwd().openFile(full_path, .{}) catch {
                try request.respond("Not Found", .{ .status = .not_found });
                continue;
            };
            defer file.close();
            const stat = try file.stat();
            const content = try file.readToEndAlloc(allocator, stat.size);
            defer allocator.free(content);
            try request.respond(content, .{ .status = .ok });
        }
    }
}
