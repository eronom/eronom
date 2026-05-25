pub mod value;
pub mod bytecode;
pub mod gc;
pub mod compiler;
pub mod execute;
pub mod jit;

// Re-export key structures to maintain public API compatibility
pub use value::Value;
pub use bytecode::{Function, Chunk, OpCode, ArrayMethodType};
pub use gc::{
    gc_allocate, gc_free_all, gc_mark_value, gc_mark_object, gc_blacken_object,
    mark_value, mark_object, gc_write_barrier,
    GcColor, GcPhase, GcData, GcObject,
    GC_HEAD, ALLOC_COUNT, GC_ROOTS, GC_PHASE, GRAY_STACK, SWEEP_PTR, PREV_SWEEP_PTR
};
pub use compiler::Compiler;
pub use execute::VM;
