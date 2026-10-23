pub use eronom_core::{alloc, bytecode, frame, gc, hooks, shape, value};

pub mod compiler;
pub mod execute;
pub mod er_http;
pub mod router;
pub mod std_fs;
pub mod std_path;
pub mod std_crypto;
pub mod std_json;
pub mod std_system;

// Re-export key structures to maintain public API compatibility
pub use eronom_core::value::Value;
pub use eronom_core::bytecode::{Function, Chunk, OpCode, ArrayMethodType};
pub use eronom_core::gc::{
    gc_allocate, gc_free_all, gc_mark_value, gc_mark_object, gc_blacken_object,
    mark_value, mark_object, gc_write_barrier, gc_epoch,
    GcColor, GcPhase, GcData, GcObject,
    GC_STATE, GC_ROOTS, GC_NEEDS_STEP
};
pub use compiler::Compiler;
pub use execute::VM;
