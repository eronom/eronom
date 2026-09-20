pub mod assignability;
pub mod checker;
pub mod env;
#[cfg(test)]
mod tests;

pub use checker::TypeChecker;
pub use env::TypeEnv;

use crate::frontend::ast::{SourceLocation, Stmt};

#[derive(Debug, Clone)]
pub struct TypeError {
    pub message: String,
    pub loc: SourceLocation,
}

impl std::fmt::Display for TypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.loc.file_path.is_empty() {
            write!(f, "Type error: {}", self.message)
        } else {
            write!(
                f,
                "Type error at {}:{}:{}: {}",
                self.loc.file_path, self.loc.line, self.loc.col, self.message
            )
        }
    }
}

impl std::error::Error for TypeError {}

pub fn check_program(stmts: &[Stmt]) -> Result<(), Vec<TypeError>> {
    let mut checker = TypeChecker::new();
    checker.check_program(stmts);
    if checker.errors.is_empty() {
        Ok(())
    } else {
        Err(checker.errors)
    }
}
