pub mod bindings;
pub mod compiler;
pub mod helpers;
pub mod profile;

// Re-export key JIT interface functions
pub use bindings::cleanup_jit;
pub use compiler::{compile_function, reset_jit_state};

pub fn init_jit_hooks() {
    crate::vm::execute::types::set_jit_hooks(
        compiler::compile_function,
        compiler::reset_jit_state,
        bindings::cleanup_jit,
        helpers::reset_global_ic,
    );
}

