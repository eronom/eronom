const std = @import("std");

pub fn main() !void {
    const T = std.process.Init;
    std.debug.print("Has deinit: {}\n", .{@hasDecl(T, "deinit")});
}
