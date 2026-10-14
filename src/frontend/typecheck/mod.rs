pub mod assignability;
pub mod checker;
pub mod env;
#[cfg(test)]
mod tests;

pub use checker::TypeChecker;
pub use env::TypeEnv;

use crate::frontend::ast::{SourceLocation, Stmt};

#[derive(Debug, Clone, serde::Serialize)]
pub struct TextEdit {
    pub line: usize,
    pub col: usize,
    pub end_line: usize,
    pub end_col: usize,
    pub new_text: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DiagnosticFix {
    pub title: String,
    pub applicability: String,
    pub edits: Vec<TextEdit>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TypeError {
    pub code: String,
    pub message: String,
    pub loc: SourceLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fixes: Vec<DiagnosticFix>,
}

impl TypeError {
    pub fn new(code: impl Into<String>, message: impl Into<String>, loc: SourceLocation) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            loc,
            explanation: None,
            fixes: Vec::new(),
        }
    }

    pub fn with_explanation(mut self, explanation: impl Into<String>) -> Self {
        self.explanation = Some(explanation.into());
        self
    }

    pub fn with_fix(mut self, fix: DiagnosticFix) -> Self {
        self.fixes.push(fix);
        self
    }
}

impl std::fmt::Display for TypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let code_prefix = if self.code.is_empty() {
            "".to_string()
        } else {
            format!("[{}] ", self.code)
        };
        if self.loc.file_path.is_empty() {
            write!(f, "{}Type error: {}", code_prefix, self.message)
        } else {
            write!(
                f,
                "{}Type error at {}:{}:{}: {}",
                code_prefix, self.loc.file_path, self.loc.line, self.loc.col, self.message
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
