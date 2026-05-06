const std = @import("std");

pub fn main() !void {
    std.debug.print("std.io has anyReader: {}\n", .{@hasDecl(std.io, "anyReader")});
}
