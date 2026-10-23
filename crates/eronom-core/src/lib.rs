pub mod alloc;
pub mod bytecode;
pub mod frame;
pub mod gc;
pub mod hooks;
pub mod shape;
pub mod value;

// Re-export key structures
pub use value::Value;
pub use bytecode::{Function, Chunk, OpCode, ArrayMethodType, UpvalueDescriptor};
pub use frame::CallFrame;
pub use gc::{
    gc_allocate, gc_free_all, gc_mark_value, gc_mark_object, gc_blacken_object,
    mark_value, mark_object, gc_write_barrier, gc_epoch,
    GcColor, GcPhase, GcData, GcObject, StructDescriptor,
    GC_STATE, GC_ROOTS, GC_NEEDS_STEP,
};
pub use shape::*;

// Internal alias so submodules can use `crate::vm::...`
pub use crate as vm;
pub use crate as core;
