// Copyright 2024 Eronom Project. All rights reserved.
// Streaming syntactic JSON token decoder with depth guard (ported from jsontext).

use std::borrow::Cow;
use super::wire::*;

pub const MAX_DEPTH: usize = 128;

#[derive(Debug, PartialEq, Clone)]
pub enum Token<'a> {
    Null,
    Bool(bool),
    Number(&'a str),
    String(Cow<'a, str>),
    BeginObject,
    EndObject,
    BeginArray,
    EndArray,
    Colon,
    Comma,
}

pub struct JsonDecoder<'a> {
    src: &'a [u8],
    pos: usize,
    depth: usize,
    peeked: Option<Token<'a>>,
}

impl<'a> JsonDecoder<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            src: input.as_bytes(),
            pos: 0,
            depth: 0,
            peeked: None,
        }
    }

    #[inline(always)]
    pub fn skip_whitespace(&mut self) {
        let ws = consume_whitespace(&self.src[self.pos..]);
        self.pos += ws;
    }

    #[inline(always)]
    pub fn is_eof(&self) -> bool {
        let mut p = self.pos;
        while p < self.src.len() {
            match self.src[p] {
                b' ' | b'\t' | b'\n' | b'\r' => p += 1,
                _ => return false,
            }
        }
        true
    }

    pub fn peek_token(&mut self) -> Result<Option<&Token<'a>>, &'static str> {
        if self.peeked.is_none() {
            self.peeked = self.read_next_token()?;
        }
        Ok(self.peeked.as_ref())
    }

    pub fn next_token(&mut self) -> Result<Option<Token<'a>>, &'static str> {
        if let Some(tok) = self.peeked.take() {
            return Ok(Some(tok));
        }
        self.read_next_token()
    }

    fn read_next_token(&mut self) -> Result<Option<Token<'a>>, &'static str> {
        self.skip_whitespace();
        if self.pos >= self.src.len() {
            return Ok(None);
        }

        let b = self.src[self.pos];
        match b {
            b'{' => {
                self.depth += 1;
                if self.depth > MAX_DEPTH {
                    return Err("Exceeded maximum JSON nesting depth");
                }
                self.pos += 1;
                Ok(Some(Token::BeginObject))
            }
            b'}' => {
                if self.depth == 0 {
                    return Err("Unexpected '}' without matching '{'");
                }
                self.depth -= 1;
                self.pos += 1;
                Ok(Some(Token::EndObject))
            }
            b'[' => {
                self.depth += 1;
                if self.depth > MAX_DEPTH {
                    return Err("Exceeded maximum JSON nesting depth");
                }
                self.pos += 1;
                Ok(Some(Token::BeginArray))
            }
            b']' => {
                if self.depth == 0 {
                    return Err("Unexpected ']' without matching '['");
                }
                self.depth -= 1;
                self.pos += 1;
                Ok(Some(Token::EndArray))
            }
            b':' => {
                self.pos += 1;
                Ok(Some(Token::Colon))
            }
            b',' => {
                self.pos += 1;
                Ok(Some(Token::Comma))
            }
            b'n' => {
                let n = consume_null(&self.src[self.pos..]);
                if n > 0 {
                    self.pos += n;
                    Ok(Some(Token::Null))
                } else {
                    Err("Invalid literal: expected 'null'")
                }
            }
            b't' | b'f' => {
                if let Some((b, len)) = consume_bool(&self.src[self.pos..]) {
                    self.pos += len;
                    Ok(Some(Token::Bool(b)))
                } else {
                    Err("Invalid literal: expected boolean 'true' or 'false'")
                }
            }
            b'"' => {
                let tok = self.read_string_token()?;
                Ok(Some(tok))
            }
            b'-' | b'0'..=b'9' => {
                let len = consume_number(&self.src[self.pos..])?;
                let slice = &self.src[self.pos..self.pos + len];
                self.pos += len;
                let num_str = std::str::from_utf8(slice)
                    .map_err(|_| "Invalid UTF-8 in numeric literal")?;
                Ok(Some(Token::Number(num_str)))
            }
            _ => Err("Unexpected character in JSON stream"),
        }
    }

    fn read_string_token(&mut self) -> Result<Token<'a>, &'static str> {
        self.pos += 1; // consume opening '"'
        let start = self.pos;
        let mut has_escape = false;

        while self.pos < self.src.len() {
            let b = self.src[self.pos];
            if b == b'"' {
                let raw_content = &self.src[start..self.pos];
                self.pos += 1; // consume closing '"'

                if !has_escape {
                    // Zero-copy borrow fast path!
                    let s = std::str::from_utf8(raw_content)
                        .map_err(|_| "Invalid UTF-8 in string literal")?;
                    return Ok(Token::String(Cow::Borrowed(s)));
                } else {
                    // Decode escapes
                    let mut unquoted = String::with_capacity(raw_content.len());
                    append_unquote(&mut unquoted, raw_content)?;
                    return Ok(Token::String(Cow::Owned(unquoted)));
                }
            } else if b == b'\\' {
                has_escape = true;
                self.pos += 2; // skip escape character + target
            } else if b < 0x20 {
                return Err("Unescaped control character in string");
            } else {
                self.pos += 1;
            }
        }

        Err("Unterminated string literal")
    }
}
