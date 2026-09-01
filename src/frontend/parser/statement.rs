use super::Parser;
use crate::frontend::token::TokenType;
use crate::frontend::ast::{Stmt, SourceLocation, SwitchCase};

impl Parser {
    pub(crate) fn statement(&mut self) -> Result<Stmt, String> {
        if self.match_token(&[TokenType::Print]) {
            self.consume(TokenType::LeftParen, "Expected '(' after print.")?;
            let value = self.expression()?;
            self.consume(TokenType::RightParen, "Expected ')' after value.")?;
            Ok(Stmt::Print(value))
        } else if self.match_token(&[TokenType::LeftBrace]) {
            self.push_scope();
            let mut stmts = Vec::new();
            while !self.check(&TokenType::RightBrace) && !self.is_at_end() {
                stmts.push(self.declaration()?);
            }
            self.consume(TokenType::RightBrace, "Expected '}' after block.")?;
            self.pop_scope();
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
        } else if self.match_token(&[TokenType::While]) {
            let has_paren = self.match_token(&[TokenType::LeftParen]);
            let condition = self.expression()?;
            if has_paren {
                self.consume(TokenType::RightParen, "Expected ')' after while condition.")?;
            }
            let body = Box::new(self.statement()?);
            Ok(Stmt::While(condition, body))
        } else if self.match_token(&[TokenType::Break]) {
            Ok(Stmt::Break)
        } else if self.match_token(&[TokenType::Continue]) {
            Ok(Stmt::Continue)
        } else if self.match_token(&[TokenType::Throw]) {
            let value = self.expression()?;
            Ok(Stmt::Throw(value))
        } else if self.match_token(&[TokenType::Try]) {
            let try_body = Box::new(self.statement()?);
            let mut catch_clause = None;
            if self.match_token(&[TokenType::Catch]) {
                let mut err_name = "err".to_string();
                if self.match_token(&[TokenType::LeftParen]) {
                    err_name = self.consume_ident("Expected variable name for catch parameter.")?;
                    self.consume(TokenType::RightParen, "Expected ')' after catch parameter.")?;
                }
                self.push_scope();
                self.declare_variable(err_name.clone());
                let catch_body = Box::new(self.statement()?);
                self.pop_scope();
                catch_clause = Some((err_name, catch_body));
            }
            let mut finally_body = None;
            if self.match_token(&[TokenType::Finally]) {
                finally_body = Some(Box::new(self.statement()?));
            }
            if catch_clause.is_none() && finally_body.is_none() {
                return Err("Expected 'catch' or 'finally' after 'try' block.".to_string());
            }
            Ok(Stmt::Try(try_body, catch_clause, finally_body))
        } else if self.match_token(&[TokenType::Switch]) {
            let has_paren = self.match_token(&[TokenType::LeftParen]);
            let target = self.expression()?;
            if has_paren {
                self.consume(TokenType::RightParen, "Expected ')' after switch target expression.")?;
            }
            self.consume(TokenType::LeftBrace, "Expected '{' after switch expression.")?;
            let mut cases = Vec::new();
            let mut default_body = None;
            while !self.check(&TokenType::RightBrace) && !self.is_at_end() {
                if self.match_token(&[TokenType::Case]) {
                    let mut values = Vec::new();
                    loop {
                        values.push(self.expression()?);
                        if !self.match_token(&[TokenType::Comma]) {
                            break;
                        }
                    }
                    self.consume(TokenType::Colon, "Expected ':' after case value(s).")?;
                    let mut case_stmts = Vec::new();
                    if self.check(&TokenType::LeftBrace) {
                        case_stmts.push(self.statement()?);
                    } else {
                        while !self.check(&TokenType::Case) && !self.check(&TokenType::Default) && !self.check(&TokenType::RightBrace) && !self.is_at_end() {
                            case_stmts.push(self.declaration()?);
                        }
                    }
                    let body = if case_stmts.len() == 1 {
                        Box::new(case_stmts.pop().unwrap())
                    } else {
                        Box::new(Stmt::Block(case_stmts))
                    };
                    cases.push(SwitchCase { values, body });
                } else if self.match_token(&[TokenType::Default]) {
                    self.consume(TokenType::Colon, "Expected ':' after default.")?;
                    let mut default_stmts = Vec::new();
                    if self.check(&TokenType::LeftBrace) {
                        default_stmts.push(self.statement()?);
                    } else {
                        while !self.check(&TokenType::Case) && !self.check(&TokenType::Default) && !self.check(&TokenType::RightBrace) && !self.is_at_end() {
                            default_stmts.push(self.declaration()?);
                        }
                    }
                    let body = if default_stmts.len() == 1 {
                        Box::new(default_stmts.pop().unwrap())
                    } else {
                        Box::new(Stmt::Block(default_stmts))
                    };
                    default_body = Some(body);
                } else {
                    return Err(format!("Error at line {}: Expected 'case' or 'default' in switch body.", self.peek().line));
                }
            }
            self.consume(TokenType::RightBrace, "Expected '}' after switch body.")?;
            Ok(Stmt::Switch(target, cases, default_body))
        } else if self.match_token(&[TokenType::Match]) {
            let has_paren = self.match_token(&[TokenType::LeftParen]);
            let target = self.expression()?;
            if has_paren {
                self.consume(TokenType::RightParen, "Expected ')' after match target expression.")?;
            }
            self.consume(TokenType::LeftBrace, "Expected '{' after match expression.")?;
            let mut cases = Vec::new();
            let mut default_body = None;
            while !self.check(&TokenType::RightBrace) && !self.is_at_end() {
                if self.match_token(&[TokenType::Underscore]) || self.match_token(&[TokenType::Default]) {
                    if !self.match_token(&[TokenType::Arrow, TokenType::Colon]) {
                        return Err(format!("Error at line {}: Expected '=>' after wildcard pattern.", self.peek().line));
                    }
                    let body = Box::new(self.statement()?);
                    self.match_token(&[TokenType::Comma]);
                    default_body = Some(body);
                } else {
                    let mut values = Vec::new();
                    loop {
                        values.push(self.expression()?);
                        if !self.match_token(&[TokenType::Comma]) {
                            break;
                        }
                    }
                    if !self.match_token(&[TokenType::Arrow, TokenType::Colon]) {
                        return Err(format!("Error at line {}: Expected '=>' after match pattern.", self.peek().line));
                    }
                    let body = Box::new(self.statement()?);
                    self.match_token(&[TokenType::Comma]);
                    cases.push(SwitchCase { values, body });
                }
            }
            self.consume(TokenType::RightBrace, "Expected '}' after match body.")?;
            Ok(Stmt::Switch(target, cases, default_body))
        } else if self.match_token(&[TokenType::For]) {
            let has_paren = self.match_token(&[TokenType::LeftParen]);
            let var_name = self.consume_ident("Expected loop variable name.")?;
            self.consume(TokenType::In, "Expected 'in' after variable name.")?;
            let first_expr = self.expression()?;
            if self.match_token(&[TokenType::DotDot]) {
                let end = self.expression()?;
                if has_paren {
                    self.consume(TokenType::RightParen, "Expected ')' after for loop range.")?;
                }
                self.push_scope();
                self.declare_variable(var_name.clone());
                let body = Box::new(self.statement()?);
                self.pop_scope();
                Ok(Stmt::For(var_name, first_expr, end, body))
            } else {
                if has_paren {
                    self.consume(TokenType::RightParen, "Expected ')' after for loop expression.")?;
                }
                self.push_scope();
                self.declare_variable(var_name.clone());
                let body = Box::new(self.statement()?);
                self.pop_scope();
                Ok(Stmt::ForIn(var_name, first_expr, body))
            }
        } else if self.match_token(&[TokenType::Return]) {
            let ret_tok = self.previous().clone();
            let value = if !self.check(&TokenType::RightBrace) && !self.is_at_end() {
                let next = &self.peek().ty;
                if next != &TokenType::RightBrace && next != &TokenType::Eof {
                    Some(self.expression()?)
                } else {
                    None
                }
            } else {
                None
            };
            let loc = SourceLocation {
                file_path: self.file_path.clone(),
                line: ret_tok.line,
                col: ret_tok.col,
            };
            Ok(Stmt::Return(value, loc))
        } else if self.match_token(&[TokenType::Concurrent]) {
            let body = Box::new(self.statement()?);
            Ok(Stmt::Concurrent(body))
        } else {
            Ok(Stmt::Expr(self.expression()?))
        }
    }
}
