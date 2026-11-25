pub mod alloc;
pub mod bytecode;
pub mod frame;
pub mod gc;
pub mod hooks;
pub mod json;
pub mod shape;
pub mod value;

// Re-export key structures
pub use alloc::{
    GcBox, GcSlabAlloc, GcVec, ShapeArenaAlloc, ShapeBox,
    alloc_box_str_in_shape_arena, alloc_box_str_in_slab, alloc_in_shape_arena, reset_shape_arena,
};
pub use value::Value;
pub use bytecode::{Function, Chunk, OpCode, ArrayMethodType, UpvalueDescriptor};
pub use frame::CallFrame;
pub use gc::{
    gc_allocate, gc_free_all, gc_mark_value, gc_mark_object, gc_blacken_object,
    mark_value, mark_object, gc_write_barrier, gc_epoch,
    gc_vec_new, gc_vec_with_capacity, gc_vec_from_slice,
    GcColor, GcPhase, GcData, GcObject, ShapePtr, StructDescriptor,
    GC_STATE, GC_ROOTS, GC_NEEDS_STEP,
};
pub use shape::*;
pub use json::{json_parse, json_stringify, escape_json_string};

// Internal alias so submodules can use `crate::vm::...`
pub use crate as vm;
pub use crate as core;
