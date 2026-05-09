const std = @import("std");
pub fn main() void {
    const heap = std.heap;
    @compileLog(@typeInfo(heap));
}
