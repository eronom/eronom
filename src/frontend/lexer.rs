use std::iter::Peekable;
use std::str::Chars;
use super::token::{Token, TokenType};

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
                        tokens.push(Token {
                            ty: TokenType::String(string.clone()),
                            line: self.line,
                        });
                        tokens.push(Token {
                            ty: TokenType::Plus,
                            line: self.line,
                        });
                        string.clear();

                        let mut expr_str = String::new();
                        while let Some(ec) = self.advance() {
                            if ec == '}' {
                                break;
                            }
                            expr_str.push(ec);
                        }

                        let mut sub_lexer = Lexer::new(&expr_str);
                        loop {
                            let tok = sub_lexer.next_token();
                            if tok.ty == TokenType::Eof {
                                break;
                            }
                            tokens.push(tok);
                        }
                        tokens.push(Token {
                            ty: TokenType::Plus,
                            line: self.line,
                        });
                    } else {
                        string.push(nc);
                    }
                }

                if tokens.is_empty() {
                    TokenType::String(string)
                } else {
                    if string.is_empty() {
                        tokens.pop();
                    } else {
                        tokens.push(Token {
                            ty: TokenType::String(string),
                            line: self.line,
                        });
                    }
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
                    "import" => TokenType::Import,
                    "export" => TokenType::Export,
                    "from" => TokenType::From,
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
