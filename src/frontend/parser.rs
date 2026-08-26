use super::token::{Token, TokenType};
use super::ast::{Expr, LiteralValue, Stmt, SourceLocation, SwitchCase};

pub struct Parser {
    tokens: Vec<Token>,
    current: usize,
    pub file_path: String,
    scopes: Vec<std::collections::HashSet<String>>,
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

    fn push_scope(&mut self) {
        self.scopes.push(std::collections::HashSet::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn declare_variable(&mut self, name: String) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name);
        }
    }

    fn is_variable_declared(&self, name: &str) -> bool {
        for scope in self.scopes.iter().rev() {
            if scope.contains(name) {
                return true;
            }
        }
        false
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
            || self.peek().ty == TokenType::Spawn
            || self.peek().ty == TokenType::On
            || self.peek().ty == TokenType::Concurrent
            || self.peek().ty == TokenType::Underscore
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
            match &tok.ty {
                TokenType::Identifier(name) => return Ok(name.clone()),
                TokenType::Spawn => return Ok("spawn".to_string()),
                TokenType::On => return Ok("on".to_string()),
                TokenType::Concurrent => return Ok("concurrent".to_string()),
                TokenType::Underscore => return Ok("_".to_string()),
                _ => {}
            }
        }
        Err(format!("Error at line {}: {}", self.peek().line, msg))
    }

    fn parse_fn_param(&mut self) -> Result<crate::frontend::ast::FnParam, String> {
        let name = self.consume_ident("Expected parameter name.")?;
        let ty = if self.match_token(&[TokenType::Colon]) {
            Some(self.consume_ident("Expected type name after ':'.")?)
        } else {
            None
        };
        Ok(crate::frontend::ast::FnParam { name, ty })
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
                                let param = self.consume_ident("Expected parameter name.")?;
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
                                let param = self.consume_ident("Expected parameter name.")?;
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

    fn statement(&mut self) -> Result<Stmt, String> {
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

    fn expression(&mut self) -> Result<Expr, String> {
        self.assignment()
    }

    fn assignment(&mut self) -> Result<Expr, String> {
        let expr = self.ternary()?;
        if self.match_token(&[
            TokenType::Equal,
            TokenType::PlusEqual,
            TokenType::MinusEqual,
            TokenType::StarEqual,
            TokenType::SlashEqual,
            TokenType::PercentEqual,
        ]) {
            let op_tok = self.previous().ty.clone();
            let value = self.assignment()?;

            let binary_op = match op_tok {
                TokenType::Equal => None,
                TokenType::PlusEqual => Some(TokenType::Plus),
                TokenType::MinusEqual => Some(TokenType::Minus),
                TokenType::StarEqual => Some(TokenType::Star),
                TokenType::SlashEqual => Some(TokenType::Slash),
                TokenType::PercentEqual => Some(TokenType::Percent),
                _ => None,
            };

            if let Some(bin_op) = binary_op {
                if let Expr::Variable(ref name, ref loc) = expr {
                    let bin_expr = Expr::Binary(
                        Box::new(Expr::Variable(name.clone(), loc.clone())),
                        bin_op,
                        Box::new(value),
                    );
                    return Ok(Expr::Assign(name.clone(), Box::new(bin_expr), loc.clone()));
                } else if let Expr::Get(ref obj, ref name) = expr {
                    let bin_expr = Expr::Binary(
                        Box::new(Expr::Get(obj.clone(), name.clone())),
                        bin_op,
                        Box::new(value),
                    );
                    return Ok(Expr::Set(obj.clone(), name.clone(), Box::new(bin_expr)));
                } else if let Expr::GetIndex(ref obj, ref index) = expr {
                    let bin_expr = Expr::Binary(
                        Box::new(Expr::GetIndex(obj.clone(), index.clone())),
                        bin_op,
                        Box::new(value),
                    );
                    return Ok(Expr::SetIndex(obj.clone(), index.clone(), Box::new(bin_expr)));
                }
                return Err("Invalid assignment target.".to_string());
            } else {
                if let Expr::Variable(name, loc) = expr {
                    return Ok(Expr::Assign(name, Box::new(value), loc));
                } else if let Expr::Get(obj, name) = expr {
                    return Ok(Expr::Set(obj, name, Box::new(value)));
                } else if let Expr::GetIndex(obj, index) = expr {
                    return Ok(Expr::SetIndex(obj, index, Box::new(value)));
                }
                return Err("Invalid assignment target.".to_string());
            }
        }
        Ok(expr)
    }

    fn ternary(&mut self) -> Result<Expr, String> {
        let mut expr = self.or()?;
        if self.match_token(&[TokenType::Question]) {
            let then_branch = self.expression()?;
            self.consume(TokenType::Colon, "Expected ':' after then expression in ternary operator.")?;
            let else_branch = self.ternary()?;
            expr = Expr::Ternary(Box::new(expr), Box::new(then_branch), Box::new(else_branch));
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
        let mut expr = self.bitwise_or()?;
        while self.match_token(&[TokenType::And]) {
            let operator = self.previous().ty.clone();
            let right = self.bitwise_or()?;
            expr = Expr::Logical(Box::new(expr), operator, Box::new(right));
        }
        Ok(expr)
    }

    fn bitwise_or(&mut self) -> Result<Expr, String> {
        let mut expr = self.bitwise_xor()?;
        while self.match_token(&[TokenType::Pipe]) {
            let operator = self.previous().ty.clone();
            let right = self.bitwise_xor()?;
            expr = Expr::Binary(Box::new(expr), operator, Box::new(right));
        }
        Ok(expr)
    }

    fn bitwise_xor(&mut self) -> Result<Expr, String> {
        let mut expr = self.bitwise_and()?;
        while self.match_token(&[TokenType::Caret]) {
            let operator = self.previous().ty.clone();
            let right = self.bitwise_and()?;
            expr = Expr::Binary(Box::new(expr), operator, Box::new(right));
        }
        Ok(expr)
    }

    fn bitwise_and(&mut self) -> Result<Expr, String> {
        let mut expr = self.equality()?;
        while self.match_token(&[TokenType::Ampersand]) {
            let operator = self.previous().ty.clone();
            let right = self.equality()?;
            expr = Expr::Binary(Box::new(expr), operator, Box::new(right));
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
        let mut expr = self.shift()?;
        while self.match_token(&[
            TokenType::Greater,
            TokenType::GreaterEqual,
            TokenType::Less,
            TokenType::LessEqual,
        ]) {
            let operator = self.previous().ty.clone();
            let right = self.shift()?;
            expr = Expr::Binary(Box::new(expr), operator, Box::new(right));
        }
        Ok(expr)
    }

    fn shift(&mut self) -> Result<Expr, String> {
        let mut expr = self.term()?;
        while self.match_token(&[TokenType::LessLess, TokenType::GreaterGreater]) {
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
        let mut expr = self.unary()?;
        while self.match_token(&[TokenType::Slash, TokenType::Star, TokenType::Percent]) {
            let operator = self.previous().ty.clone();
            let right = self.unary()?;
            expr = Expr::Binary(Box::new(expr), operator, Box::new(right));
        }
        Ok(expr)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.match_token(&[TokenType::Bang, TokenType::Minus, TokenType::Tilde, TokenType::Typeof]) {
            let operator = self.previous().ty.clone();
            let expr = self.unary()?;
            return Ok(Expr::Unary(operator, Box::new(expr)));
        }
        if self.match_token(&[TokenType::PlusPlus, TokenType::MinusMinus]) {
            let operator = self.previous().ty.clone();
            let expr = self.unary()?;
            return Ok(Expr::Prefix(operator, Box::new(expr)));
        }
        if self.match_token(&[TokenType::Spawn, TokenType::On]) {
            let expr = self.call()?;
            return Ok(Expr::Spawn(Box::new(expr)));
        }
        self.call()
    }

    fn call(&mut self) -> Result<Expr, String> {
        let mut expr = self.primary()?;
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
            } else if self.match_token(&[TokenType::PlusPlus, TokenType::MinusMinus]) {
                let operator = self.previous().ty.clone();
                expr = Expr::Postfix(operator, Box::new(expr));
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn primary(&mut self) -> Result<Expr, String> {
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
                    Expr::Variable(name, _) => vec![crate::frontend::ast::FnParam { name, ty: None }],
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

pub fn parse_and_resolve_imports(path: &std::path::Path) -> Result<Vec<Stmt>, String> {
    let mut visited = std::collections::HashSet::new();
    let mut visited_exports = std::collections::HashMap::new();
    resolve_imports_recursive(path, &mut visited, &mut visited_exports)
}

fn get_exported_names(stmts: &[Stmt]) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    for stmt in stmts {
        if let Stmt::Export(inner) = stmt {
            match &**inner {
                Stmt::VarDecl(name, _, _, _, _) => {
                    names.insert(name.clone());
                }
                Stmt::Struct(name, _, _, _, _) => {
                    names.insert(name.clone());
                }
                Stmt::Interface(name, _, _, _) => {
                    names.insert(name.clone());
                }
                _ => {}
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
    let path_str = path.to_string_lossy().to_string();

    let (canonical, content) = if path.exists() {
        let canonical = path.canonicalize()
            .map_err(|e| format!("Failed to canonicalize path {:?}: {}", path, e))?;
        let content = std::fs::read_to_string(&canonical)
            .map_err(|e| format!("Failed to read file {:?}: {}", canonical, e))?;
        (canonical, content)
    } else if let Some(vfs_text) = crate::vm::embedded::get_vfs_text(&path_str) {
        (std::path::PathBuf::from(&path_str), vfs_text)
    } else {
        return Err(format!("File not found on disk or in embedded VFS: {:?}", path));
    };

    if visited.contains(&canonical) {
        return Ok(Vec::new());
    }
    visited.insert(canonical.clone());

    let tokens = super::lexer::lex(&content);
    let mut parser = Parser::new(tokens).with_file_path(canonical.to_string_lossy().to_string());
    let stmts = parser.parse()?;

    // Populate visited_exports for this file with its direct exports
    let direct_exports = get_exported_names(&stmts);
    visited_exports.insert(canonical.clone(), direct_exports);

    let mut resolved_stmts = Vec::new();
    let parent_dir = canonical.parent().unwrap_or(std::path::Path::new(""));

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
                
                // Fallbacks on disk
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

                // Check VFS if not on disk
                let resolved_path_str = resolved_path.to_string_lossy().to_string();
                let is_in_vfs = !resolved_path.exists() && (
                    crate::vm::embedded::has_vfs_file(&resolved_path_str) ||
                    crate::vm::embedded::has_vfs_file(&import_path) ||
                    (import_path.ends_with(".js") && crate::vm::embedded::has_vfs_file(&import_path.replace(".js", ".er"))) ||
                    (!import_path.ends_with(".er") && crate::vm::embedded::has_vfs_file(&format!("{}.er", import_path))) ||
                    (is_std_import && !import_path.ends_with(".er") && crate::vm::embedded::has_vfs_file(&format!("{}.er", import_path)))
                );

                if !resolved_path.exists() && !is_in_vfs {
                    return Err(format!(
                        "Imported file not found: {:?} (specified as {})",
                        resolved_path, import_path
                    ));
                }

                let final_path = if is_in_vfs {
                    if crate::vm::embedded::has_vfs_file(&resolved_path_str) {
                        resolved_path
                    } else if crate::vm::embedded::has_vfs_file(&import_path) {
                        std::path::PathBuf::from(&import_path)
                    } else if import_path.ends_with(".js") && crate::vm::embedded::has_vfs_file(&import_path.replace(".js", ".er")) {
                        std::path::PathBuf::from(import_path.replace(".js", ".er"))
                    } else if !import_path.ends_with(".er") && crate::vm::embedded::has_vfs_file(&format!("{}.er", import_path)) {
                        std::path::PathBuf::from(format!("{}.er", import_path))
                    } else {
                        resolved_path
                    }
                } else {
                    resolved_path.canonicalize()
                        .map_err(|e| format!("Failed to canonicalize path {:?}: {}", resolved_path, e))?
                };

                let sub_stmts = if visited.contains(&final_path) {
                    Vec::new()
                } else {
                    resolve_imports_recursive(&final_path, visited, visited_exports)?
                };
                
                let exports = visited_exports.get(&final_path).cloned().unwrap_or_default();
                for name in &names {
                    if !exports.contains(name) {
                        return Err(format!(
                            "Name '{}' is not exported by {:?}",
                            name, final_path
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

