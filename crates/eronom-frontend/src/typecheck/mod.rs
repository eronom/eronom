pub mod assignability;
pub mod checker;
pub mod env;
#[cfg(test)]
mod tests;

pub use checker::TypeChecker;
pub use env::TypeEnv;

use crate::frontend::ast::{SourceLocation, Stmt};

#[derive(Debug, Clone)]
pub struct TextEdit {
    pub line: usize,
    pub col: usize,
    pub end_line: usize,
    pub end_col: usize,
    pub new_text: String,
}

impl TextEdit {
    pub fn to_json(&self) -> String {
        let mut text_esc = String::new();
        crate::frontend::ast::escape_json_string(&mut text_esc, &self.new_text);
        format!(
            "{{\"line\":{},\"col\":{},\"end_line\":{},\"end_col\":{},\"new_text\":\"{}\"}}",
            self.line, self.col, self.end_line, self.end_col, text_esc
        )
    }
}

#[derive(Debug, Clone)]
pub struct DiagnosticFix {
    pub title: String,
    pub applicability: String,
    pub edits: Vec<TextEdit>,
}

impl DiagnosticFix {
    pub fn to_json(&self) -> String {
        let mut title_esc = String::new();
        let mut app_esc = String::new();
        crate::frontend::ast::escape_json_string(&mut title_esc, &self.title);
        crate::frontend::ast::escape_json_string(&mut app_esc, &self.applicability);
        let mut out = format!(
            "{{\"title\":\"{}\",\"applicability\":\"{}\",\"edits\":[",
            title_esc, app_esc
        );
        for (i, edit) in self.edits.iter().enumerate() {
            if i > 0 { out.push(','); }
            out.push_str(&edit.to_json());
        }
        out.push_str("]}");
        out
    }
}

#[derive(Debug, Clone)]
pub struct TypeError {
    pub code: String,
    pub message: String,
    pub loc: SourceLocation,
    pub explanation: Option<String>,
    pub fixes: Vec<DiagnosticFix>,
}

impl TypeError {
    pub fn to_json(&self) -> String {
        let mut code_esc = String::new();
        let mut msg_esc = String::new();
        crate::frontend::ast::escape_json_string(&mut code_esc, &self.code);
        crate::frontend::ast::escape_json_string(&mut msg_esc, &self.message);
        let mut out = format!(
            "{{\"code\":\"{}\",\"message\":\"{}\",\"loc\":{}",
            code_esc, msg_esc, self.loc.to_json()
        );
        if let Some(ref exp) = self.explanation {
            let mut exp_esc = String::new();
            crate::frontend::ast::escape_json_string(&mut exp_esc, exp);
            out.push_str(&format!(",\"explanation\":\"{}\"", exp_esc));
        }
        if !self.fixes.is_empty() {
            out.push_str(",\"fixes\":[");
            for (i, fix) in self.fixes.iter().enumerate() {
                if i > 0 { out.push(','); }
                out.push_str(&fix.to_json());
            }
            out.push(']');
        }
        out.push('}');
        out
    }
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
