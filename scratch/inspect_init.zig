const std = @import("std");

pub fn main() !void {
    const T = std.process.Init;
    inline for (std.meta.fields(T)) |field| {
        std.debug.print("Field: {s}\n", .{field.name});
    }
}
