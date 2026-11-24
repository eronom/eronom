#![allow(clippy::not_unsafe_ptr_arg_deref)]

pub use eronom_frontend as frontend;
pub use eronom_vm as vm;
pub mod jit;
pub mod toolchain;
pub mod cli;
pub mod runner;


#[path = "../libs/erm/mod.rs"]
pub mod erm;

pub use erm::compiler;
pub use erm::eval;
pub use erm::server;
pub use erm::router;
