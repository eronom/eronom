const std = @import("std");
const eval = @import("../src/eval.zig");

pub fn main() !void {
    const val = eval.Value{ .number = 42.0 };

    var buf: [128]u8 = undefined;
    const s = try std.fmt.bufPrint(&buf, "{f}", .{val});

    std.debug.print("Formatted: {s}\n", .{s});

    if (!std.mem.eql(u8, s, "42")) {
        std.debug.print("FAILURE: expected '42', got '{s}'\n", .{s});
        std.process.exit(1);
    }
}
