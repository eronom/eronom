mod declaration;
mod statement;
mod expression;
mod primary;
mod resolver;

use super::token::{Token, TokenType};
use super::ast::Stmt;

pub use resolver::parse_and_resolve_imports;

pub struct Parser {
    pub(crate) tokens: Vec<Token>,
    pub(crate) current: usize,
    pub file_path: String,
    pub(crate) scopes: Vec<std::collections::HashSet<String>>,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            current: 0,
            file_path: "".to_string(),
            scopes: vec![std::collections::HashSet::new()],
        }
    }

    pub fn with_file_path(mut self, path: String) -> Self {
        self.file_path = path;
        self
    }

    pub(crate) fn push_scope(&mut self) {
        self.scopes.push(std::collections::HashSet::new());
    }

    pub(crate) fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    pub(crate) fn declare_variable(&mut self, name: String) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name);
        }
    }

    pub(crate) fn is_variable_declared(&self, name: &str) -> bool {
        for scope in self.scopes.iter().rev() {
            if scope.contains(name) {
                return true;
            }
        }
        false
    }

    pub(crate) fn peek(&self) -> &Token {
        &self.tokens[self.current]
    }

    pub(crate) fn previous(&self) -> &Token {
        &self.tokens[self.current - 1]
    }

    pub(crate) fn is_at_end(&self) -> bool {
        self.peek().ty == TokenType::Eof
    }

    pub(crate) fn advance(&mut self) -> &Token {
        if !self.is_at_end() {
            self.current += 1;
        }
        self.previous()
    }

    pub(crate) fn check(&self, ty: &TokenType) -> bool {
        if self.is_at_end() {
            return false;
        }
        &self.peek().ty == ty
    }

    pub(crate) fn check_ident(&self) -> bool {
        if self.is_at_end() {
            return false;
        }
        matches!(self.peek().ty, TokenType::Identifier(_))
            || self.peek().ty == TokenType::Spawn
            || self.peek().ty == TokenType::On
            || self.peek().ty == TokenType::Concurrent
            || self.peek().ty == TokenType::Underscore
            || self.peek().ty == TokenType::From
            || self.peek().ty == TokenType::Default
            || self.peek().ty == TokenType::Typeof
    }

    pub(crate) fn match_token(&mut self, types: &[TokenType]) -> bool {
        for ty in types {
            if self.check(ty) {
                self.advance();
                return true;
            }
        }
        false
    }

    pub(crate) fn consume(&mut self, ty: TokenType, msg: &str) -> Result<&Token, String> {
        if self.check(&ty) {
            Ok(self.advance())
        } else {
            Err(format!("Error at line {}: {}", self.peek().line, msg))
        }
    }

    pub(crate) fn consume_ident(&mut self, msg: &str) -> Result<String, String> {
        if self.check_ident() {
            let tok = self.advance();
            match &tok.ty {
                TokenType::Identifier(name) => return Ok(name.clone()),
                TokenType::Spawn => return Ok("spawn".to_string()),
                TokenType::On => return Ok("on".to_string()),
                TokenType::Concurrent => return Ok("concurrent".to_string()),
                TokenType::Underscore => return Ok("_".to_string()),
                TokenType::From => return Ok("from".to_string()),
                TokenType::Default => return Ok("default".to_string()),
                TokenType::Typeof => return Ok("typeof".to_string()),
                _ => {}
            }
        }
        Err(format!("Error at line {}: {}", self.peek().line, msg))
    }

    pub fn parse(&mut self) -> Result<Vec<Stmt>, String> {
        let mut statements = Vec::new();
        while !self.is_at_end() {
            statements.push(self.declaration()?);
        }
        Ok(statements)
    }
}
