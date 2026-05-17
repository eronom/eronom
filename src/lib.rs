//! By convention, lib.rs is the root source file when making a library.

pub use er;
pub mod compiler;
pub mod eval;
pub mod router;

use std::io::{self, Write};

pub fn buffered_print() -> io::Result<()> {
    // Stdout is for the actual output of your application, for example if you
    // are implementing gzip, then only the compressed bytes should be sent to
    // stdout, not any debugging messages.
    let mut stdout = io::BufWriter::new(io::stdout());

    writeln!(stdout, "Run `cargo test` to run the tests.")?;

    stdout.flush()?; // Don't forget to flush!
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
