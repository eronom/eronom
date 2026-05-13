const std = @import("std");
pub fn main() !void {
    const T = std.heap;
    inline for (std.meta.declarations(T)) |decl| {
        std.debug.print("decl: {s}\n", .{decl.name});
    }
}
