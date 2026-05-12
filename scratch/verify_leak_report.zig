const std = @import("std");
pub fn main() !void {
    var gpa: std.heap.DebugAllocator(.{}) = .init;
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();
    _ = try allocator.alloc(u8, 100);
    std.debug.print("Leaked 100 bytes!\n", .{});
}
