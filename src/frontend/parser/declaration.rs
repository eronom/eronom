use super::Parser;
use crate::frontend::token::TokenType;
use crate::frontend::ast::{Expr, LiteralValue, Stmt, SourceLocation, FnParam};

impl Parser {
    pub(crate) fn parse_fn_param(&mut self) -> Result<FnParam, String> {
        let name = self.consume_ident("Expected parameter name.")?;
        let ty = if self.match_token(&[TokenType::Colon]) {
            Some(self.consume_ident("Expected type name after ':'.")?)
        } else {
            None
        };
        Ok(FnParam { name, ty })
    }

    pub(crate) fn declaration(&mut self) -> Result<Stmt, String> {
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
                for name in &names {
                    self.declare_variable(name.clone());
                }
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
        } else if self.match_token(&[TokenType::Struct]) {
            let name_tok = self.peek().clone();
            let name = self.consume_ident("Expected struct name.")?;
            self.declare_variable(name.clone());
            let mut composed = Vec::new();
            if self.match_token(&[TokenType::Embed]) {
                loop {
                    let parent = self.consume_ident("Expected parent struct name after 'embed'.")?;
                    composed.push(parent);
                    if !self.match_token(&[TokenType::Comma]) {
                        break;
                    }
                }
            }
            self.consume(TokenType::LeftBrace, "Expected '{' before struct body.")?;
            let mut fields = Vec::new();
            let mut methods = Vec::new();
            if !self.check(&TokenType::RightBrace) {
                loop {
                    if self.match_token(&[TokenType::Function]) {
                        let method_name = self.consume_ident("Expected method name.")?;
                        self.consume(TokenType::LeftParen, "Expected '(' after method name.")?;
                        let mut params = Vec::new();
                        if !self.check(&TokenType::RightParen) {
                            loop {
                                let param = self.parse_fn_param()?.name;
                                params.push(param);
                                if !self.match_token(&[TokenType::Comma]) {
                                    break;
                                }
                            }
                        }
                        self.consume(TokenType::RightParen, "Expected ')' after parameters.")?;
                        self.push_scope();
                        self.declare_variable("this".to_string());
                        for param in &params {
                            self.declare_variable(param.clone());
                        }
                        self.consume(TokenType::LeftBrace, "Expected '{' before method body.")?;
                        let mut stmts = Vec::new();
                        while !self.check(&TokenType::RightBrace) && !self.is_at_end() {
                            stmts.push(self.declaration()?);
                        }
                        self.consume(TokenType::RightBrace, "Expected '}' after method body.")?;
                        let body = Stmt::Block(stmts);
                        self.pop_scope();
                        methods.push((method_name, params, body));
                    } else {
                        let field_name = self.consume_ident("Expected field name.")?;
                        self.consume(TokenType::Colon, "Expected ':' after field name.")?;
                        let field_type = self.consume_ident("Expected field type.")?;
                        fields.push((field_name, field_type));
                    }
                    self.match_token(&[TokenType::Comma]);
                    if self.check(&TokenType::RightBrace) || self.is_at_end() {
                        break;
                    }
                }
            }
            self.consume(TokenType::RightBrace, "Expected '}' after struct body.")?;
            let loc = SourceLocation {
                file_path: self.file_path.clone(),
                line: name_tok.line,
                col: name_tok.col,
            };
            Ok(Stmt::Struct(name, composed, fields, methods, loc))
        } else if self.match_token(&[TokenType::Interface]) {
            let name_tok = self.peek().clone();
            let name = self.consume_ident("Expected interface name.")?;
            self.declare_variable(name.clone());
            self.consume(TokenType::LeftBrace, "Expected '{' before interface body.")?;
            let mut fields = Vec::new();
            let mut methods = Vec::new();
            if !self.check(&TokenType::RightBrace) {
                loop {
                    if self.match_token(&[TokenType::Function]) {
                        let method_name = self.consume_ident("Expected method name.")?;
                        self.consume(TokenType::LeftParen, "Expected '(' after method name.")?;
                        let mut params = Vec::new();
                        if !self.check(&TokenType::RightParen) {
                            loop {
                                let param = self.parse_fn_param()?.name;
                                params.push(param);
                                if !self.match_token(&[TokenType::Comma]) {
                                    break;
                                }
                            }
                        }
                        self.consume(TokenType::RightParen, "Expected ')' after parameters.")?;
                        methods.push((method_name, params));
                    } else {
                        let field_name = self.consume_ident("Expected field name.")?;
                        self.consume(TokenType::Colon, "Expected ':' after field name.")?;
                        let field_type = self.consume_ident("Expected field type.")?;
                        fields.push((field_name, field_type));
                    }
                    self.match_token(&[TokenType::Comma]);
                    if self.check(&TokenType::RightBrace) || self.is_at_end() {
                        break;
                    }
                }
            }
            self.consume(TokenType::RightBrace, "Expected '}' after interface body.")?;
            let loc = SourceLocation {
                file_path: self.file_path.clone(),
                line: name_tok.line,
                col: name_tok.col,
            };
            Ok(Stmt::Interface(name, fields, methods, loc))
        } else if self.match_token(&[TokenType::Let, TokenType::Const]) {
            let is_const = self.previous().ty == TokenType::Const;
            let name_tok = self.peek().clone();
            let name = self.consume_ident("Expected variable name.")?;

            // Skip optional type annotation like `: string`
            let mut type_annotation = None;
            if self.match_token(&[TokenType::Colon]) {
                type_annotation = Some(self.consume_ident("Expected type name after ':'.")?);
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
            self.declare_variable(name.clone());
            Ok(Stmt::VarDecl(name, type_annotation, is_const, initializer, loc))
        } else if self.match_token(&[TokenType::Function]) {
            let name_tok = self.peek().clone();
            let name = self.consume_ident("Expected function name.")?;
            self.declare_variable(name.clone());
            self.consume(TokenType::LeftParen, "Expected '(' after function name.")?;
            let mut params = Vec::new();
            if !self.check(&TokenType::RightParen) {
                loop {
                    let param = self.parse_fn_param()?;
                    params.push(param);
                    if !self.match_token(&[TokenType::Comma]) {
                        break;
                    }
                }
            }
            self.consume(TokenType::RightParen, "Expected ')' after parameters.")?;
            let return_type = if self.match_token(&[TokenType::Colon]) {
                Some(self.consume_ident("Expected return type name after ':'.")?)
            } else {
                None
            };
            self.push_scope();
            for param in &params {
                self.declare_variable(param.name.clone());
            }
            let body = if self.match_token(&[TokenType::Arrow]) {
                let body_stmt = self.statement()?;
                let body_loc = SourceLocation {
                    file_path: self.file_path.clone(),
                    line: name_tok.line,
                    col: name_tok.col,
                };
                match body_stmt {
                    Stmt::Expr(expr) => Stmt::Return(Some(expr), body_loc),
                    other => other,
                }
            } else {
                self.consume(TokenType::LeftBrace, "Expected '{' before function body.")?;
                let mut stmts = Vec::new();
                while !self.check(&TokenType::RightBrace) && !self.is_at_end() {
                    stmts.push(self.declaration()?);
                }
                self.consume(TokenType::RightBrace, "Expected '}' after function body.")?;
                Stmt::Block(stmts)
            };
            self.pop_scope();
            let loc = SourceLocation {
                file_path: self.file_path.clone(),
                line: name_tok.line,
                col: name_tok.col,
            };
            Ok(Stmt::VarDecl(name, None, false, Expr::Function(params, return_type, Box::new(body)), loc))
        } else if self.check_ident() && {
            // Check for assignment or short variable declaration: ident = expr
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
            if self.is_variable_declared(&name) {
                Ok(Stmt::Expr(Expr::Assign(name, Box::new(expr), loc)))
            } else {
                self.declare_variable(name.clone());
                Ok(Stmt::VarDecl(name, None, false, expr, loc))
            }
        } else {
            self.statement()
        }
    }
}
