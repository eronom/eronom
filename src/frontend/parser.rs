use super::token::{Token, TokenType};
use super::ast::{Expr, LiteralValue, Stmt};

pub struct Parser {
    tokens: Vec<Token>,
    current: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, current: 0 }
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
        if self.match_token(&[TokenType::Let, TokenType::Const]) {
            let is_const = self.previous().ty == TokenType::Const;
            let name = self.consume_ident("Expected variable name.")?;

            // Skip optional type annotation like `: string`
            if self.match_token(&[TokenType::Colon]) {
                self.consume_ident("Expected type name after ':'.")?;
            }

            let mut initializer = Expr::Literal(LiteralValue::Null);
            if self.match_token(&[TokenType::Equal]) {
                initializer = self.expression()?;
            }
            Ok(Stmt::VarDecl(name, is_const, initializer))
        } else if self.check_ident() && {
            // Check for simple assignment without let/const: ident = expr
            self.current + 1 < self.tokens.len()
                && self.tokens[self.current + 1].ty == TokenType::Equal
        } {
            let name = self.consume_ident("Expected variable name.")?;
            self.advance(); // consume =
            let expr = self.expression()?;
            Ok(Stmt::Expr(Expr::Assign(name, Box::new(expr))))
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
            if let Expr::Variable(name) = expr {
                return Ok(Expr::Assign(name, Box::new(value)));
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

    fn call(&mut self) -> Result<Expr, String> {
        let mut expr = self.primary()?;
        loop {
            if self.match_token(&[TokenType::LeftParen]) {
                let mut args = Vec::new();
                if !self.check(&TokenType::RightParen) {
                    loop {
                        args.push(self.expression()?);
                        if !self.match_token(&[TokenType::Comma]) {
                            break;
                        }
                    }
                }
                self.consume(TokenType::RightParen, "Expected ')' after arguments.")?;
                expr = Expr::Call(Box::new(expr), args);
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
            let name = self.consume_ident("Expected identifier")?;
            return Ok(Expr::Variable(name));
        }

        if self.match_token(&[TokenType::LeftParen]) {
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

        Err(format!("Unexpected token: {:?}", self.peek().ty))
    }
}
