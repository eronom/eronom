mod declaration;
mod statement;
mod expression;
mod primary;
mod resolver;
mod type_parser;

use super::token::{Token, TokenType};
use super::ast::Stmt;

pub use resolver::{parse_and_resolve_imports, get_imported_file_paths};

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

    pub(crate) fn is_async_function_or_arrow(&self) -> bool {
        if self.peek().ty != TokenType::Async {
            return false;
        }
        let next_ty = self.tokens.get(self.current + 1).map(|t| &t.ty);
        let next_next_ty = self.tokens.get(self.current + 2).map(|t| &t.ty);
        matches!(next_ty, Some(TokenType::Function) | Some(TokenType::LeftParen))
            || (matches!(next_ty, Some(TokenType::Identifier(_))) && matches!(next_next_ty, Some(TokenType::Arrow)))
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
            || (self.peek().ty == TokenType::Async && !self.is_async_function_or_arrow())
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
                TokenType::Async => return Ok("async".to_string()),
                _ => {}
            }
        }
        Err(format!("Error at line {}: {}", self.peek().line, msg))
    }

    pub(crate) fn consume_property_name(&mut self) -> Result<String, String> {
        let tok = self.peek().clone();
        let name = match &tok.ty {
            TokenType::Identifier(s) => s.clone(),
            TokenType::Number(n) => n.to_string(),
            TokenType::Type => "type".to_string(),
            TokenType::Await => "await".to_string(),
            TokenType::New => "new".to_string(),
            TokenType::Spawn => "spawn".to_string(),
            TokenType::On => "on".to_string(),
            TokenType::Concurrent => "concurrent".to_string(),
            TokenType::Underscore => "_".to_string(),
            TokenType::From => "from".to_string(),
            TokenType::Default => "default".to_string(),
            TokenType::Typeof => "typeof".to_string(),
            TokenType::Async => "async".to_string(),
            TokenType::Sync => "sync".to_string(),
            TokenType::As => "as".to_string(),
            TokenType::Is => "is".to_string(),
            TokenType::In => "in".to_string(),
            TokenType::For => "for".to_string(),
            TokenType::If => "if".to_string(),
            TokenType::Else => "else".to_string(),
            TokenType::While => "while".to_string(),
            TokenType::Break => "break".to_string(),
            TokenType::Continue => "continue".to_string(),
            TokenType::Return => "return".to_string(),
            TokenType::Function => "function".to_string(),
            TokenType::Struct => "struct".to_string(),
            TokenType::Embed => "embed".to_string(),
            TokenType::Interface => "interface".to_string(),
            TokenType::Try => "try".to_string(),
            TokenType::Catch => "catch".to_string(),
            TokenType::Finally => "finally".to_string(),
            TokenType::Throw => "throw".to_string(),
            TokenType::Switch => "switch".to_string(),
            TokenType::Case => "case".to_string(),
            TokenType::Match => "match".to_string(),
            TokenType::Enum => "enum".to_string(),
            TokenType::Null => "null".to_string(),
            TokenType::True => "true".to_string(),
            TokenType::False => "false".to_string(),
            TokenType::Print => "print".to_string(),
            TokenType::Let => "let".to_string(),
            TokenType::Const => "const".to_string(),
            TokenType::Import => "import".to_string(),
            TokenType::Export => "export".to_string(),
            _ => {
                return Err(format!(
                    "Error at line {}: Expected property name or number after '.'.",
                    tok.line
                ));
            }
        };
        self.advance();
        Ok(name)
    }

    pub(crate) fn consume_prop_key(&mut self, msg: &str) -> Result<String, String> {
        if let TokenType::String(s) = &self.peek().ty {
            let s_clone = s.clone();
            self.advance();
            return Ok(s_clone);
        }
        self.consume_property_name().map_err(|_| format!("Error at line {}: {}", self.peek().line, msg))
    }

    pub fn parse(&mut self) -> Result<Vec<Stmt>, String> {
        let mut statements = Vec::new();
        while !self.is_at_end() {
            statements.push(self.declaration()?);
        }
        Ok(statements)
    }
}
