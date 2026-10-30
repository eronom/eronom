pub mod helpers;
pub mod profile;
pub mod clif;

// Re-export key JIT interface functions
pub use clif::{
    clif_compile_function as compile_function,
    clif_reset_jit_state as reset_jit_state,
    clif_cleanup_jit as cleanup_jit,
};

pub fn init_jit_hooks() {
    crate::vm::execute::types::set_jit_hooks(
        clif::clif_compile_function,
        clif::clif_reset_jit_state,
        clif::clif_cleanup_jit,
        helpers::reset_global_ic,
    );
}

