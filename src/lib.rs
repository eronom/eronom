pub mod frontend;
pub mod vm;
pub mod jit;
pub mod toolchain;
pub mod cli;

#[path = "../libs/erm/mod.rs"]
pub mod erm;

pub use erm::compiler;
pub use erm::server;
pub use erm::eval;
pub use erm::router;

pub fn buffered_print() -> std::io::Result<()> {
    let mut stdout = std::io::BufWriter::new(std::io::stdout());
    use std::io::Write;
    writeln!(stdout, "Run `cargo test` to run the tests.")?;
    stdout.flush()?;
    Ok(())
}

pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_add_functionality() {
        assert_eq!(add(3, 7), 10);
    }
}
