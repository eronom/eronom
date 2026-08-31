use super::Parser;
use crate::frontend::token::TokenType;
use crate::frontend::ast::Expr;

impl Parser {
    pub(crate) fn expression(&mut self) -> Result<Expr, String> {
        self.assignment()
    }

    pub(crate) fn assignment(&mut self) -> Result<Expr, String> {
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

    pub(crate) fn ternary(&mut self) -> Result<Expr, String> {
        let mut expr = self.or()?;
        if self.match_token(&[TokenType::Question]) {
            let then_branch = self.expression()?;
            self.consume(TokenType::Colon, "Expected ':' after then expression in ternary operator.")?;
            let else_branch = self.ternary()?;
            expr = Expr::Ternary(Box::new(expr), Box::new(then_branch), Box::new(else_branch));
        }
        Ok(expr)
    }

    pub(crate) fn or(&mut self) -> Result<Expr, String> {
        let mut expr = self.and()?;
        while self.match_token(&[TokenType::Or]) {
            let operator = self.previous().ty.clone();
            let right = self.and()?;
            expr = Expr::Logical(Box::new(expr), operator, Box::new(right));
        }
        Ok(expr)
    }

    pub(crate) fn and(&mut self) -> Result<Expr, String> {
        let mut expr = self.bitwise_or()?;
        while self.match_token(&[TokenType::And]) {
            let operator = self.previous().ty.clone();
            let right = self.bitwise_or()?;
            expr = Expr::Logical(Box::new(expr), operator, Box::new(right));
        }
        Ok(expr)
    }

    pub(crate) fn bitwise_or(&mut self) -> Result<Expr, String> {
        let mut expr = self.bitwise_xor()?;
        while self.match_token(&[TokenType::Pipe]) {
            let operator = self.previous().ty.clone();
            let right = self.bitwise_xor()?;
            expr = Expr::Binary(Box::new(expr), operator, Box::new(right));
        }
        Ok(expr)
    }

    pub(crate) fn bitwise_xor(&mut self) -> Result<Expr, String> {
        let mut expr = self.bitwise_and()?;
        while self.match_token(&[TokenType::Caret]) {
            let operator = self.previous().ty.clone();
            let right = self.bitwise_and()?;
            expr = Expr::Binary(Box::new(expr), operator, Box::new(right));
        }
        Ok(expr)
    }

    pub(crate) fn bitwise_and(&mut self) -> Result<Expr, String> {
        let mut expr = self.equality()?;
        while self.match_token(&[TokenType::Ampersand]) {
            let operator = self.previous().ty.clone();
            let right = self.equality()?;
            expr = Expr::Binary(Box::new(expr), operator, Box::new(right));
        }
        Ok(expr)
    }

    pub(crate) fn equality(&mut self) -> Result<Expr, String> {
        let mut expr = self.comparison()?;
        while self.match_token(&[TokenType::BangEqual, TokenType::EqualEqual]) {
            let operator = self.previous().ty.clone();
            let right = self.comparison()?;
            expr = Expr::Binary(Box::new(expr), operator, Box::new(right));
        }
        Ok(expr)
    }

    pub(crate) fn comparison(&mut self) -> Result<Expr, String> {
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

    pub(crate) fn shift(&mut self) -> Result<Expr, String> {
        let mut expr = self.term()?;
        while self.match_token(&[TokenType::LessLess, TokenType::GreaterGreater]) {
            let operator = self.previous().ty.clone();
            let right = self.term()?;
            expr = Expr::Binary(Box::new(expr), operator, Box::new(right));
        }
        Ok(expr)
    }

    pub(crate) fn term(&mut self) -> Result<Expr, String> {
        let mut expr = self.factor()?;
        while self.match_token(&[TokenType::Minus, TokenType::Plus]) {
            let operator = self.previous().ty.clone();
            let right = self.factor()?;
            expr = Expr::Binary(Box::new(expr), operator, Box::new(right));
        }
        Ok(expr)
    }

    pub(crate) fn factor(&mut self) -> Result<Expr, String> {
        let mut expr = self.unary()?;
        while self.match_token(&[TokenType::Slash, TokenType::Star, TokenType::Percent]) {
            let operator = self.previous().ty.clone();
            let right = self.unary()?;
            expr = Expr::Binary(Box::new(expr), operator, Box::new(right));
        }
        Ok(expr)
    }

    pub(crate) fn unary(&mut self) -> Result<Expr, String> {
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

    pub(crate) fn call(&mut self) -> Result<Expr, String> {
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
}
