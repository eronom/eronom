use super::token::{Token, TokenType};
use super::ast::{Expr, LiteralValue, Stmt, SourceLocation};

pub struct Parser {
    tokens: Vec<Token>,
    current: usize,
    pub file_path: String,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, current: 0, file_path: "".to_string() }
    }

    pub fn with_file_path(mut self, path: String) -> Self {
        self.file_path = path;
        self
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.current]
    }

    fn previous(&self) -> &Token {
        &self.tokens[self.current - 1]
    }

    fn is_at_end(&self) -> bool {
        self.peek().ty == TokenType::Eof
    }

    fn advance(&mut self) -> &Token {
        if !self.is_at_end() {
            self.current += 1;
        }
        self.previous()
    }

    fn check(&self, ty: &TokenType) -> bool {
        if self.is_at_end() {
            return false;
        }
        &self.peek().ty == ty
    }

    fn check_ident(&self) -> bool {
        if self.is_at_end() {
            return false;
        }
        matches!(self.peek().ty, TokenType::Identifier(_))
    }

    fn match_token(&mut self, types: &[TokenType]) -> bool {
        for ty in types {
            if self.check(ty) {
                self.advance();
                return true;
            }
        }
        false
    }

    fn consume(&mut self, ty: TokenType, msg: &str) -> Result<&Token, String> {
        if self.check(&ty) {
            Ok(self.advance())
        } else {
            Err(format!("Error at line {}: {}", self.peek().line, msg))
        }
    }

    fn consume_ident(&mut self, msg: &str) -> Result<String, String> {
        if self.check_ident() {
            let tok = self.advance();
            if let TokenType::Identifier(name) = &tok.ty {
                return Ok(name.clone());
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

    fn declaration(&mut self) -> Result<Stmt, String> {
        if self.match_token(&[TokenType::Import]) {
            if self.match_token(&[TokenType::LeftBrace]) {
                let mut names = Vec::new();
                if !self.check(&TokenType::RightBrace) {
                    loop {
                        let name = self.consume_ident("Expected identifier in import list.")?;
                        names.push(name);
                        if !self.match_token(&[TokenType::Comma]) {
                            break;
                        }
                    }
                }
                self.consume(TokenType::RightBrace, "Expected '}' after import list.")?;
                self.consume(TokenType::From, "Expected 'from' after import list.")?;
                let path_token = self.peek().clone();
                let path = if let TokenType::String(s) = path_token.ty {
                    self.advance();
                    s
                } else {
                    return Err(format!("Error at line {}: Expected string literal for import path.", path_token.line));
                };
                Ok(Stmt::Import(names, path))
            } else {
                let path_token = self.peek().clone();
                let path = if let TokenType::String(s) = path_token.ty {
                    self.advance();
                    s
                } else {
                    return Err(format!("Error at line {}: Expected string literal or '{{' after import.", path_token.line));
                };
                Ok(Stmt::Import(Vec::new(), path))
            }
        } else if self.match_token(&[TokenType::Export]) {
            let decl = self.declaration()?;
            Ok(Stmt::Export(Box::new(decl)))
        } else if self.match_token(&[TokenType::Let, TokenType::Const]) {
            let is_const = self.previous().ty == TokenType::Const;
            let name_tok = self.peek().clone();
            let name = self.consume_ident("Expected variable name.")?;

            // Skip optional type annotation like `: string`
            if self.match_token(&[TokenType::Colon]) {
                self.consume_ident("Expected type name after ':'.")?;
            }

            let mut initializer = Expr::Literal(LiteralValue::Null);
            if self.match_token(&[TokenType::Equal]) {
                initializer = self.expression()?;
            }
            let loc = SourceLocation {
                file_path: self.file_path.clone(),
                line: name_tok.line,
                col: name_tok.col,
            };
            Ok(Stmt::VarDecl(name, is_const, initializer, loc))
        } else if self.match_token(&[TokenType::Function]) {
            let name_tok = self.peek().clone();
            let name = self.consume_ident("Expected function name.")?;
            self.consume(TokenType::LeftParen, "Expected '(' after function name.")?;
            let mut params = Vec::new();
            if !self.check(&TokenType::RightParen) {
                loop {
                    let param = self.consume_ident("Expected parameter name.")?;
                    params.push(param);
                    if !self.match_token(&[TokenType::Comma]) {
                        break;
                    }
                }
            }
            self.consume(TokenType::RightParen, "Expected ')' after parameters.")?;
            self.consume(TokenType::LeftBrace, "Expected '{' before function body.")?;
            let mut stmts = Vec::new();
            while !self.check(&TokenType::RightBrace) && !self.is_at_end() {
                stmts.push(self.declaration()?);
            }
            self.consume(TokenType::RightBrace, "Expected '}' after function body.")?;
            let body = Stmt::Block(stmts);
            let loc = SourceLocation {
                file_path: self.file_path.clone(),
                line: name_tok.line,
                col: name_tok.col,
            };
            Ok(Stmt::VarDecl(name, false, Expr::Function(params, Box::new(body)), loc))
        } else if self.check_ident() && {
            // Check for simple assignment without let/const: ident = expr
            self.current + 1 < self.tokens.len()
                && self.tokens[self.current + 1].ty == TokenType::Equal
        } {
            let name_tok = self.peek().clone();
            let name = self.consume_ident("Expected variable name.")?;
            self.advance(); // consume =
            let expr = self.expression()?;
            let loc = SourceLocation {
                file_path: self.file_path.clone(),
                line: name_tok.line,
                col: name_tok.col,
            };
            Ok(Stmt::Expr(Expr::Assign(name, Box::new(expr), loc)))
        } else {
            self.statement()
        }
    }

    fn statement(&mut self) -> Result<Stmt, String> {
        if self.match_token(&[TokenType::Print]) {
            self.consume(TokenType::LeftParen, "Expected '(' after print.")?;
            let value = self.expression()?;
            self.consume(TokenType::RightParen, "Expected ')' after value.")?;
            Ok(Stmt::Print(value))
        } else if self.match_token(&[TokenType::LeftBrace]) {
            let mut stmts = Vec::new();
            while !self.check(&TokenType::RightBrace) && !self.is_at_end() {
                stmts.push(self.declaration()?);
            }
            self.consume(TokenType::RightBrace, "Expected '}' after block.")?;
            Ok(Stmt::Block(stmts))
        } else if self.match_token(&[TokenType::If]) {
            self.consume(TokenType::LeftParen, "Expected '(' after if.")?;
            let condition = self.expression()?;
            self.consume(TokenType::RightParen, "Expected ')' after condition.")?;
            let then_branch = Box::new(self.statement()?);
            let else_branch = if self.match_token(&[TokenType::Else]) {
                Some(Box::new(self.statement()?))
            } else {
                None
            };
            Ok(Stmt::If(condition, then_branch, else_branch))
        } else if self.match_token(&[TokenType::For]) {
            let var_name = self.consume_ident("Expected loop variable name.")?;
            self.consume(TokenType::In, "Expected 'in' after variable name.")?;
            let start = self.expression()?;
            self.consume(TokenType::DotDot, "Expected '..' in for loop range.")?;
            let end = self.expression()?;
            let body = Box::new(self.statement()?);
            Ok(Stmt::For(var_name, start, end, body))
        } else if self.match_token(&[TokenType::Return]) {
            let value = if !self.check(&TokenType::RightBrace) && !self.is_at_end() {
                // simple check
                // Might have return value
                // In a proper parser we check if there's a semicolon or a block end.
                // We'll just try to parse an expression if we are not at end of block.
                let next = &self.peek().ty;
                if next != &TokenType::RightBrace && next != &TokenType::Eof {
                    Some(self.expression()?)
                } else {
                    None
                }
            } else {
                None
            };
            Ok(Stmt::Return(value))
        } else {
            Ok(Stmt::Expr(self.expression()?))
        }
    }

    fn expression(&mut self) -> Result<Expr, String> {
        self.assignment()
    }

    fn assignment(&mut self) -> Result<Expr, String> {
        let expr = self.or()?;
        if self.match_token(&[TokenType::Equal]) {
            let value = self.assignment()?;
            if let Expr::Variable(name, loc) = expr {
                return Ok(Expr::Assign(name, Box::new(value), loc));
            } else if let Expr::Get(obj, name) = expr {
                return Ok(Expr::Set(obj, name, Box::new(value)));
            } else if let Expr::GetIndex(obj, index) = expr {
                return Ok(Expr::SetIndex(obj, index, Box::new(value)));
            }
            return Err("Invalid assignment target.".to_string());
        }
        Ok(expr)
    }

    fn or(&mut self) -> Result<Expr, String> {
        let mut expr = self.and()?;
        while self.match_token(&[TokenType::Or]) {
            let operator = self.previous().ty.clone();
            let right = self.and()?;
            expr = Expr::Logical(Box::new(expr), operator, Box::new(right));
        }
        Ok(expr)
    }

    fn and(&mut self) -> Result<Expr, String> {
        let mut expr = self.equality()?;
        while self.match_token(&[TokenType::And]) {
            let operator = self.previous().ty.clone();
            let right = self.equality()?;
            expr = Expr::Logical(Box::new(expr), operator, Box::new(right));
        }
        Ok(expr)
    }

    fn equality(&mut self) -> Result<Expr, String> {
        let mut expr = self.comparison()?;
        while self.match_token(&[TokenType::BangEqual, TokenType::EqualEqual]) {
            let operator = self.previous().ty.clone();
            let right = self.comparison()?;
            expr = Expr::Binary(Box::new(expr), operator, Box::new(right));
        }
        Ok(expr)
    }

    fn comparison(&mut self) -> Result<Expr, String> {
        let mut expr = self.term()?;
        while self.match_token(&[
            TokenType::Greater,
            TokenType::GreaterEqual,
            TokenType::Less,
            TokenType::LessEqual,
        ]) {
            let operator = self.previous().ty.clone();
            let right = self.term()?;
            expr = Expr::Binary(Box::new(expr), operator, Box::new(right));
        }
        Ok(expr)
    }

    fn term(&mut self) -> Result<Expr, String> {
        let mut expr = self.factor()?;
        while self.match_token(&[TokenType::Minus, TokenType::Plus]) {
            let operator = self.previous().ty.clone();
            let right = self.factor()?;
            expr = Expr::Binary(Box::new(expr), operator, Box::new(right));
        }
        Ok(expr)
    }

    fn factor(&mut self) -> Result<Expr, String> {
        let mut expr = self.call()?;
        while self.match_token(&[TokenType::Slash, TokenType::Star]) {
            let operator = self.previous().ty.clone();
            let right = self.call()?;
            expr = Expr::Binary(Box::new(expr), operator, Box::new(right));
        }
        Ok(expr)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        self.primary()
    }

    fn call(&mut self) -> Result<Expr, String> {
        let mut expr = self.unary()?;
        loop {
            if self.match_token(&[TokenType::LeftParen]) {
                let mut arguments = Vec::new();
                if !self.check(&TokenType::RightParen) {
                    loop {
                        arguments.push(self.expression()?);
                        if !self.match_token(&[TokenType::Comma]) {
                            break;
                        }
                    }
                }
                self.consume(TokenType::RightParen, "Expected ')' after arguments.")?;
                expr = Expr::Call(Box::new(expr), arguments);
            } else if self.match_token(&[TokenType::Dot]) {
                let name = if self.check_ident() {
                    self.consume_ident("Expected property name.")?
                } else if let TokenType::Number(n) = self.peek().ty.clone() {
                    self.advance();
                    n.to_string()
                } else {
                    return Err(format!(
                        "Error at line {}: Expected property name or number after '.'.",
                        self.peek().line
                    ));
                };
                expr = Expr::Get(Box::new(expr), name);
            } else if self.match_token(&[TokenType::LeftBracket]) {
                let index = self.expression()?;
                self.consume(TokenType::RightBracket, "Expected ']' after index.")?;
                expr = Expr::GetIndex(Box::new(expr), Box::new(index));
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn primary(&mut self) -> Result<Expr, String> {
        if self.match_token(&[TokenType::Function]) {
            let _name = if self.check_ident() {
                Some(self.consume_ident("Expected function name.")?)
            } else {
                None
            };
            self.consume(TokenType::LeftParen, "Expected '(' after function keyword.")?;
            let mut params = Vec::new();
            if !self.check(&TokenType::RightParen) {
                loop {
                    let param = self.consume_ident("Expected parameter name.")?;
                    params.push(param);
                    if !self.match_token(&[TokenType::Comma]) {
                        break;
                    }
                }
            }
            self.consume(TokenType::RightParen, "Expected ')' after parameters.")?;
            self.consume(TokenType::LeftBrace, "Expected '{' before function body.")?;
            let mut stmts = Vec::new();
            while !self.check(&TokenType::RightBrace) && !self.is_at_end() {
                stmts.push(self.declaration()?);
            }
            self.consume(TokenType::RightBrace, "Expected '}' after function body.")?;
            let body = Stmt::Block(stmts);
            return Ok(Expr::Function(params, Box::new(body)));
        }

        if self.match_token(&[TokenType::False]) {
            return Ok(Expr::Literal(LiteralValue::Boolean(false)));
        }
        if self.match_token(&[TokenType::True]) {
            return Ok(Expr::Literal(LiteralValue::Boolean(true)));
        }
        if self.match_token(&[TokenType::Null]) {
            return Ok(Expr::Literal(LiteralValue::Null));
        }

        if let TokenType::Number(n) = self.peek().ty.clone() {
            self.advance();
            return Ok(Expr::Literal(LiteralValue::Number(n)));
        }

        if let TokenType::String(s) = self.peek().ty.clone() {
            self.advance();
            return Ok(Expr::Literal(LiteralValue::String(s)));
        }

        if self.check_ident() {
            let tok = self.peek().clone();
            let name = self.consume_ident("Expected identifier")?;
            let loc = SourceLocation {
                file_path: self.file_path.clone(),
                line: tok.line,
                col: tok.col,
            };
            return Ok(Expr::Variable(name, loc));
        }

        if self.match_token(&[TokenType::LeftParen]) {
            if self.check(&TokenType::RightParen) {
                let temp = self.current;
                if temp + 1 < self.tokens.len() && self.tokens[temp + 1].ty == TokenType::Arrow {
                    self.advance(); // consume RightParen
                    self.advance(); // consume Arrow
                    let body = self.statement()?;
                    return Ok(Expr::Function(Vec::new(), Box::new(body)));
                }
            }
            let mut params = Vec::new();
            if self.check_ident() {
                let save_pos = self.current;
                let mut is_arrow = false;

                if !self.check(&TokenType::RightParen) {
                    loop {
                        if let Ok(name) = self.consume_ident("Expected parameter name") {
                            params.push(name);
                        } else {
                            break;
                        }
                        if !self.match_token(&[TokenType::Comma]) {
                            break;
                        }
                    }
                }

                if self.match_token(&[TokenType::RightParen])
                    && self.match_token(&[TokenType::Arrow])
                {
                    is_arrow = true;
                }

                if is_arrow {
                    let body = self.statement()?;
                    return Ok(Expr::Function(params, Box::new(body)));
                } else {
                    self.current = save_pos;
                }
            }

            let expr = self.expression()?;
            self.consume(TokenType::RightParen, "Expected ')' after expression.")?;

            if self.match_token(&[TokenType::Arrow]) {
                let body = self.statement()?;
                return Ok(Expr::Function(vec![], Box::new(body)));
            }

            return Ok(expr);
        }

        if self.match_token(&[TokenType::LeftBracket]) {
            let mut items = Vec::new();
            if !self.check(&TokenType::RightBracket) {
                loop {
                    items.push(self.expression()?);
                    if !self.match_token(&[TokenType::Comma]) {
                        break;
                    }
                }
            }
            self.consume(
                TokenType::RightBracket,
                "Expected ']' after array elements.",
            )?;
            return Ok(Expr::Array(items));
        }

        if self.match_token(&[TokenType::LeftBrace]) {
            let mut pairs = Vec::new();
            if !self.check(&TokenType::RightBrace) {
                loop {
                    let key = self.consume_ident("Expected property key.")?;
                    self.consume(TokenType::Colon, "Expected ':' after property key.")?;
                    let value = self.expression()?;
                    pairs.push((key, value));
                    if !self.match_token(&[TokenType::Comma]) {
                        break;
                    }
                }
            }
            self.consume(
                TokenType::RightBrace,
                "Expected '}' after object properties.",
            )?;
            return Ok(Expr::Object(pairs));
        }

        Err(format!("Error at line {}: Unexpected token: {:?}", self.peek().line, self.peek().ty))
    }
}

pub fn parse_and_resolve_imports(path: &std::path::Path) -> Result<Vec<Stmt>, String> {
    let mut visited = std::collections::HashSet::new();
    let mut visited_exports = std::collections::HashMap::new();
    resolve_imports_recursive(path, &mut visited, &mut visited_exports)
}

fn get_exported_names(stmts: &[Stmt]) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    for stmt in stmts {
        if let Stmt::Export(inner) = stmt {
            if let Stmt::VarDecl(name, _, _, _) = &**inner {
                names.insert(name.clone());
            }
        }
    }
    names
}

fn find_std_dir(start_dir: &std::path::Path) -> Option<std::path::PathBuf> {
    // 1. Search upwards from the compiling file's directory
    let mut current = Some(start_dir);
    while let Some(dir) = current {
        let std_dir = dir.join("std");
        if std_dir.is_dir() {
            return Some(std_dir);
        }
        current = dir.parent();
    }

    // 2. Search upwards from current working directory
    if let Ok(cwd) = std::env::current_dir() {
        let mut current = Some(cwd.as_path());
        while let Some(dir) = current {
            let std_dir = dir.join("std");
            if std_dir.is_dir() {
                return Some(std_dir);
            }
            current = dir.parent();
        }
    }

    // 3. Search relative to executable
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let mut current = Some(exe_dir);
            while let Some(dir) = current {
                let std_dir = dir.join("std");
                if std_dir.is_dir() {
                    return Some(std_dir);
                }
                current = dir.parent();
            }
        }
    }

    None
}

fn resolve_imports_recursive(
    path: &std::path::Path,
    visited: &mut std::collections::HashSet<std::path::PathBuf>,
    visited_exports: &mut std::collections::HashMap<std::path::PathBuf, std::collections::HashSet<String>>,
) -> Result<Vec<Stmt>, String> {
    let canonical = path.canonicalize()
        .map_err(|e| format!("Failed to canonicalize path {:?}: {}", path, e))?;

    if visited.contains(&canonical) {
        return Ok(Vec::new());
    }
    visited.insert(canonical.clone());

    let content = std::fs::read_to_string(&canonical)
        .map_err(|e| format!("Failed to read file {:?}: {}", canonical, e))?;

    let tokens = super::lexer::lex(&content);
    let mut parser = Parser::new(tokens).with_file_path(canonical.to_string_lossy().to_string());
    let stmts = parser.parse()?;

    // Populate visited_exports for this file with its direct exports
    let direct_exports = get_exported_names(&stmts);
    visited_exports.insert(canonical.clone(), direct_exports);

    let mut resolved_stmts = Vec::new();
    let parent_dir = canonical.parent().ok_or_else(|| "No parent directory".to_string())?;

    for stmt in stmts {
        match stmt {
            Stmt::Import(names, import_path) => {
                let is_std_import = import_path.starts_with("std/") || import_path == "std" || import_path.starts_with("std\\");
                let mut resolved_path = if is_std_import {
                    if let Some(std_root) = find_std_dir(parent_dir) {
                        let std_parent = std_root.parent().unwrap_or(&std_root);
                        std_parent.join(&import_path)
                    } else {
                        parent_dir.join(&import_path)
                    }
                } else {
                    parent_dir.join(&import_path)
                };
                
                // Fallbacks:
                if !resolved_path.exists() {
                    if import_path.ends_with(".js") {
                        let er_path = resolved_path.with_extension("er");
                        if er_path.exists() {
                            resolved_path = er_path;
                        }
                    }
                }
                
                if !resolved_path.exists() {
                    let er_path = resolved_path.with_extension("er");
                    if er_path.exists() {
                        resolved_path = er_path;
                    }
                }

                if !resolved_path.exists() {
                    return Err(format!(
                        "Imported file not found: {:?} (specified as {})",
                        resolved_path, import_path
                    ));
                }

                let resolved_canonical = resolved_path.canonicalize()
                    .map_err(|e| format!("Failed to canonicalize path {:?}: {}", resolved_path, e))?;

                let sub_stmts = if visited.contains(&resolved_canonical) {
                    Vec::new()
                } else {
                    resolve_imports_recursive(&resolved_path, visited, visited_exports)?
                };
                
                let exports = visited_exports.get(&resolved_canonical).cloned().unwrap_or_default();
                for name in &names {
                    if !exports.contains(name) {
                        return Err(format!(
                            "Name '{}' is not exported by {:?}",
                            name, resolved_path
                        ));
                    }
                }
                
                resolved_stmts.extend(sub_stmts);
            }
            Stmt::Export(inner) => {
                resolved_stmts.push(Stmt::Export(inner));
            }
            _ => {
                resolved_stmts.push(stmt);
            }
        }
    }

    Ok(resolved_stmts)
}
