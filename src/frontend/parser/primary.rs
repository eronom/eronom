use super::Parser;
use crate::frontend::token::TokenType;
use crate::frontend::ast::{Expr, LiteralValue, Stmt, SourceLocation, FnParam};

impl Parser {
    pub(crate) fn primary(&mut self) -> Result<Expr, String> {
        if self.match_token(&[TokenType::Function]) {
            let name = if self.check_ident() {
                Some(self.consume_ident("Expected function name.")?)
            } else {
                None
            };
            self.consume(TokenType::LeftParen, "Expected '(' after function keyword.")?;
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
            if let Some(ref n) = name {
                self.declare_variable(n.clone());
            }
            for param in &params {
                self.declare_variable(param.name.clone());
            }
            let body = if self.match_token(&[TokenType::Arrow]) {
                let arrow_tok = self.previous().clone();
                let body_stmt = self.statement()?;
                let body_loc = SourceLocation {
                    file_path: self.file_path.clone(),
                    line: arrow_tok.line,
                    col: arrow_tok.col,
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
            return Ok(Expr::Function(params, return_type, Box::new(body)));
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
            if self.match_token(&[TokenType::Arrow]) {
                let arrow_tok = self.previous().clone();
                self.push_scope();
                self.declare_variable(name.clone());
                let body = self.statement()?;
                let body_loc = SourceLocation {
                    file_path: self.file_path.clone(),
                    line: arrow_tok.line,
                    col: arrow_tok.col,
                };
                let body = match body {
                    Stmt::Expr(expr) => Stmt::Return(Some(expr), body_loc),
                    other => other,
                };
                self.pop_scope();
                let params = vec![FnParam { name, ty: None }];
                return Ok(Expr::Function(params, None, Box::new(body)));
            }
            let is_capitalized = name.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false);
            let next_is_prop = self.check(&TokenType::LeftBrace)
                && (self.current + 1 < self.tokens.len()
                    && (self.tokens[self.current + 1].ty == TokenType::RightBrace
                        || (matches!(self.tokens[self.current + 1].ty, TokenType::Identifier(_))
                            && self.current + 2 < self.tokens.len()
                            && self.tokens[self.current + 2].ty == TokenType::Colon)));

            if is_capitalized && next_is_prop && self.match_token(&[TokenType::LeftBrace]) {
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
                        if self.check(&TokenType::RightBrace) {
                            break;
                        }
                    }
                }
                self.consume(
                    TokenType::RightBrace,
                    "Expected '}' after struct properties.",
                )?;
                return Ok(Expr::StructInst(name, pairs, loc));
            }
            return Ok(Expr::Variable(name, loc));
        }

        if self.match_token(&[TokenType::LeftParen]) {
            let save_pos = self.current;
            let mut params = Vec::new();
            let mut is_arrow = false;
            let mut return_type = None;

            if self.match_token(&[TokenType::RightParen]) {
                if self.match_token(&[TokenType::Colon]) {
                    if let Ok(rt) = self.consume_ident("Expected return type") {
                        return_type = Some(rt);
                    }
                }
                if self.match_token(&[TokenType::Arrow]) {
                    is_arrow = true;
                }
            } else if self.check_ident() {
                let mut valid_params = true;
                loop {
                    match self.parse_fn_param() {
                        Ok(p) => params.push(p),
                        Err(_) => {
                            valid_params = false;
                            break;
                        }
                    }
                    if !self.match_token(&[TokenType::Comma]) {
                        break;
                    }
                }
                if valid_params && self.match_token(&[TokenType::RightParen]) {
                    if self.match_token(&[TokenType::Colon]) {
                        if let Ok(rt) = self.consume_ident("Expected return type") {
                            return_type = Some(rt);
                        }
                    }
                    if self.match_token(&[TokenType::Arrow]) {
                        is_arrow = true;
                    }
                }
            }

            if is_arrow {
                let arrow_tok = self.previous().clone();
                self.push_scope();
                for param in &params {
                    self.declare_variable(param.name.clone());
                }
                let body = self.statement()?;
                let body_loc = SourceLocation {
                    file_path: self.file_path.clone(),
                    line: arrow_tok.line,
                    col: arrow_tok.col,
                };
                let body = match body {
                    Stmt::Expr(expr) => Stmt::Return(Some(expr), body_loc),
                    other => other,
                };
                self.pop_scope();
                return Ok(Expr::Function(params, return_type, Box::new(body)));
            } else {
                self.current = save_pos;
            }

            let expr = self.expression()?;
            self.consume(TokenType::RightParen, "Expected ')' after expression.")?;

            let return_type = if self.match_token(&[TokenType::Colon]) {
                Some(self.consume_ident("Expected return type")?)
            } else {
                None
            };

            if self.match_token(&[TokenType::Arrow]) {
                let arrow_tok = self.previous().clone();
                self.push_scope();
                let body = self.statement()?;
                let body_loc = SourceLocation {
                    file_path: self.file_path.clone(),
                    line: arrow_tok.line,
                    col: arrow_tok.col,
                };
                let body = match body {
                    Stmt::Expr(expr) => Stmt::Return(Some(expr), body_loc),
                    other => other,
                };
                self.pop_scope();
                let params = match expr {
                    Expr::Variable(name, _) => vec![FnParam { name, ty: None }],
                    _ => vec![],
                };
                return Ok(Expr::Function(params, return_type, Box::new(body)));
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
                    if self.check(&TokenType::RightBrace) {
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
