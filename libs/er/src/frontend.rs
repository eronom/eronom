use std::iter::Peekable;
use std::str::Chars;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenType {
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    Comma,
    Dot,
    Minus,
    Plus,
    Colon,
    Slash,
    Star,
    Bang,
    BangEqual,
    Equal,
    EqualEqual,
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
    Arrow,
    DotDot,
    Identifier(String),
    String(String),
    Number(f64),
    And,
    Else,
    False,
    For,
    If,
    Null,
    Or,
    Print,
    Return,
    True,
    Let,
    Const,
    In,
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub ty: TokenType,
    pub line: usize,
}

pub struct Lexer<'a> {
    chars: Peekable<Chars<'a>>,
    line: usize,
    buffer: Vec<Token>,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            chars: source.chars().peekable(),
            line: 1,
            buffer: Vec::new(),
        }
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.chars.next();
        if c == Some('\n') {
            self.line += 1;
        }
        c
    }

    fn peek(&mut self) -> Option<&char> {
        self.chars.peek()
    }

    fn match_char(&mut self, expected: char) -> bool {
        if let Some(&c) = self.peek() {
            if c == expected {
                self.advance();
                return true;
            }
        }
        false
    }

    fn skip_whitespace(&mut self) {
        while let Some(&c) = self.peek() {
            if c.is_whitespace() || c == ';' {
                self.advance();
            } else if c == '/' {
                // Peek ahead to see if it's a comment
                let mut temp = self.chars.clone();
                temp.next();
                if temp.next() == Some('/') {
                    while let Some(&c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.advance();
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }

    pub fn next_token(&mut self) -> Token {
        if let Some(tok) = self.buffer.pop() {
            return tok;
        }

        self.skip_whitespace();
        let line = self.line;

        let c = match self.advance() {
            Some(c) => c,
            None => {
                return Token {
                    ty: TokenType::Eof,
                    line,
                };
            }
        };

        let ty = match c {
            '(' => TokenType::LeftParen,
            ')' => TokenType::RightParen,
            '{' => TokenType::LeftBrace,
            '}' => TokenType::RightBrace,
            '[' => TokenType::LeftBracket,
            ']' => TokenType::RightBracket,
            ',' => TokenType::Comma,
            ':' => TokenType::Colon,
            '-' => TokenType::Minus,
            '+' => TokenType::Plus,
            '*' => TokenType::Star,
            '/' => TokenType::Slash,
            '.' => {
                if self.match_char('.') {
                    TokenType::DotDot
                } else {
                    TokenType::Dot
                }
            }
            '!' => {
                if self.match_char('=') {
                    TokenType::BangEqual
                } else {
                    TokenType::Bang
                }
            }
            '=' => {
                if self.match_char('=') {
                    TokenType::EqualEqual
                } else if self.match_char('>') {
                    TokenType::Arrow
                } else {
                    TokenType::Equal
                }
            }
            '<' => {
                if self.match_char('=') {
                    TokenType::LessEqual
                } else {
                    TokenType::Less
                }
            }
            '>' => {
                if self.match_char('=') {
                    TokenType::GreaterEqual
                } else {
                    TokenType::Greater
                }
            }
            '"' | '\'' => {
                let quote = c;
                let mut string = String::new();
                let mut tokens = Vec::new();
                
                while let Some(nc) = self.advance() {
                    if nc == quote {
                        break;
                    }
                    if nc == '{' {
                        tokens.push(Token { ty: TokenType::String(string.clone()), line: self.line });
                        tokens.push(Token { ty: TokenType::Plus, line: self.line });
                        string.clear();
                        
                        let mut expr_str = String::new();
                        while let Some(ec) = self.advance() {
                            if ec == '}' { break; }
                            expr_str.push(ec);
                        }
                        
                        let mut sub_lexer = Lexer::new(&expr_str);
                        loop {
                            let tok = sub_lexer.next_token();
                            if tok.ty == TokenType::Eof { break; }
                            tokens.push(tok);
                        }
                        tokens.push(Token { ty: TokenType::Plus, line: self.line });
                    } else {
                        string.push(nc);
                    }
                }
                
                if tokens.is_empty() {
                    TokenType::String(string)
                } else {
                    tokens.push(Token { ty: TokenType::String(string), line: self.line });
                    // To output multiple tokens, we can store them in the lexer's buffer.
                    // Let's add a buffer to the Lexer struct! Wait, I can't easily change the Lexer struct here without also changing its definition.
                    // Instead, let's just make the parent loop handle this, or return a special InterpolatedString token?
                    // Let's change the lexer to use a buffer.
                    self.buffer.extend(tokens.into_iter().rev());
                    let first = self.buffer.pop().unwrap();
                    return first;
                }
            }
            _ if c.is_ascii_digit() => {
                let mut num = String::from(c);
                while let Some(&nc) = self.peek() {
                    if nc.is_ascii_digit() {
                        num.push(self.advance().unwrap());
                    } else if nc == '.' {
                        // Check if it's the `..` operator
                        let mut temp = self.chars.clone();
                        temp.next(); // Consume the peeked '.'
                        if temp.peek() == Some(&'.') {
                            break;
                        }
                        num.push(self.advance().unwrap());
                    } else {
                        break;
                    }
                }
                TokenType::Number(num.parse().unwrap_or(0.0))
            }
            _ if c.is_ascii_alphabetic() || c == '_' => {
                let mut ident = String::from(c);
                while let Some(&nc) = self.peek() {
                    if nc.is_ascii_alphanumeric() || nc == '_' {
                        ident.push(self.advance().unwrap());
                    } else {
                        break;
                    }
                }
                match ident.as_str() {
                    "and" => TokenType::And,
                    "else" => TokenType::Else,
                    "false" => TokenType::False,
                    "for" => TokenType::For,
                    "if" => TokenType::If,
                    "null" => TokenType::Null,
                    "or" => TokenType::Or,
                    "print" => TokenType::Print,
                    "return" => TokenType::Return,
                    "true" => TokenType::True,
                    "let" => TokenType::Let,
                    "const" => TokenType::Const,
                    "in" => TokenType::In,
                    _ => TokenType::Identifier(ident),
                }
            }
            _ => TokenType::Eof, // Or an error token, but for simplicity...
        };

        Token { ty, line }
    }
}

pub fn lex(source: &str) -> Vec<Token> {
    let mut lexer = Lexer::new(source);
    let mut tokens = Vec::new();
    loop {
        let tok = lexer.next_token();
        let is_eof = tok.ty == TokenType::Eof;
        tokens.push(tok);
        if is_eof {
            break;
        }
    }
    tokens
}

#[derive(Debug, Clone)]
pub enum Expr {
    Literal(LiteralValue),
    Variable(String),
    Assign(String, Box<Expr>),
    Binary(Box<Expr>, TokenType, Box<Expr>),
    Logical(Box<Expr>, TokenType, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    Get(Box<Expr>, String),
    Set(Box<Expr>, String, Box<Expr>),
    Array(Vec<Expr>),
    Object(Vec<(String, Expr)>),
    Function(Vec<String>, Box<Stmt>),
    GetIndex(Box<Expr>, Box<Expr>),
    SetIndex(Box<Expr>, Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone)]
pub enum LiteralValue {
    Number(f64),
    String(String),
    Boolean(bool),
    Null,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Expr(Expr),
    Print(Expr),
    VarDecl(String, bool, Expr), // name, is_const, initializer
    Block(Vec<Stmt>),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    For(String, Expr, Expr, Box<Stmt>), // var, start, end, body
    Return(Option<Expr>),
}

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
                    return Err(format!("Error at line {}: Expected property name or number after '.'.", self.peek().line));
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
            // Might be arrow function `(x, y) => { ... }` or just a grouped expr
            let mut params = Vec::new();
            if self.check_ident() {
                // To do this simply, we parse args, but if we see '=>', it's a function.
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
                    // Backtrack
                    self.current = save_pos;
                }
            }

            // Otherwise, normal grouped expr
            let expr = self.expression()?;
            self.consume(TokenType::RightParen, "Expected ')' after expression.")?;

            // Check for arrow function `(expr) => ...` (wait, standard arrow is handled above, but what if no args: `() => {}`)
            if self.match_token(&[TokenType::Arrow]) {
                let body = self.statement()?;
                return Ok(Expr::Function(vec![], Box::new(body))); // No params
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
                    let _ = self.match_token(&[TokenType::Comma]);
                    if self.check(&TokenType::RightBrace) || self.is_at_end() {
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
