const std = @import("std");
pub fn main() void {
    const heap = std.heap;
    inline for (std.meta.declarations(heap)) |decl| {
        @compileLog(decl.name);
    }
}
