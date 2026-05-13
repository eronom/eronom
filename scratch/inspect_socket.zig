const std = @import("std");
pub fn main() !void {
    const T = std.Io.net.Socket;
    inline for (std.meta.fields(T)) |f| {
        std.debug.print("field: {s} type: {any}\n", .{f.name, f.type});
    }
}
