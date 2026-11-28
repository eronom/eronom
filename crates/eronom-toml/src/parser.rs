use std::borrow::Cow;

use crate::error::Error;
use crate::value::{Table, Value};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Scalar<'a> {
    String(Cow<'a, str>),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Datetime(&'a str),
}

impl<'a> Scalar<'a> {
    fn into_value(self) -> Value {
        match self {
            Scalar::String(s) => Value::String(s.into_owned()),
            Scalar::Integer(i) => Value::Integer(i),
            Scalar::Float(f) => Value::Float(f),
            Scalar::Boolean(b) => Value::Boolean(b),
            Scalar::Datetime(d) => Value::Datetime(d.to_owned()),
        }
    }
}

pub(crate) fn parse_document(input: &str) -> Result<Table, Error> {
    let mut parser = Parser::new(input);
    let mut root = Table::new();
    parser.parse_root_into(&mut root, None)?;
    Ok(root)
}

pub(crate) fn lookup_scalar<'a>(input: &'a str, target: &[&str]) -> Result<Option<Scalar<'a>>, Error> {
    if target.is_empty() {
        return Ok(None);
    }
    let mut parser = Parser::new(input);
    let mut found = None;
    let mut dummy_root = Table::new();
    parser.parse_root_into(&mut dummy_root, Some((target, &mut found)))?;
    Ok(found)
}

struct Parser<'a> {
    input: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            bytes: input.as_bytes(),
            pos: 0,
        }
    }

    #[cold]
    fn err(&self, message: impl Into<String>) -> Error {
        let prefix = &self.input[..self.pos];
        let mut line = 1usize;
        let mut last_nl = 0usize;
        for (idx, b) in prefix.bytes().enumerate() {
            if b == b'\n' {
                line += 1;
                last_nl = idx + 1;
            }
        }
        let col = prefix[last_nl..].chars().count() + 1;
        Error::new(line, col, message)
    }

    #[inline]
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    #[inline]
    fn starts_with(&self, pat: &[u8]) -> bool {
        self.bytes[self.pos..].starts_with(pat)
    }

    #[inline]
    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if b == b' ' || b == b'\t' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn skip_comment_body(&mut self) {
        while let Some(b) = self.peek() {
            if b == b'\n' || b == b'\r' {
                break;
            }
            self.pos += 1;
        }
    }

    fn skip_ws_and_newlines(&mut self) {
        loop {
            while let Some(b) = self.peek() {
                if matches!(b, b' ' | b'\t' | b'\n' | b'\r') {
                    self.pos += 1;
                } else {
                    break;
                }
            }
            if self.peek() == Some(b'#') {
                self.pos += 1;
                self.skip_comment_body();
            } else {
                break;
            }
        }
    }

    fn expect_line_end(&mut self) -> Result<(), Error> {
        self.skip_ws();
        if self.peek() == Some(b'#') {
            self.pos += 1;
            self.skip_comment_body();
        }
        match self.peek() {
            None => Ok(()),
            Some(b'\n') => {
                self.pos += 1;
                Ok(())
            }
            Some(b'\r') => {
                self.pos += 1;
                if self.peek() == Some(b'\n') {
                    self.pos += 1;
                }
                Ok(())
            }
            Some(_) => Err(self.err("expected newline or comment")),
        }
    }

    fn parse_root_into(
        &mut self,
        root: &mut Table,
        mut target: Option<(&[&str], &mut Option<Scalar<'a>>)>,
    ) -> Result<(), Error> {
        let mut current_table: Vec<Cow<'a, str>> = Vec::new();
        let mut key_buf: Vec<Cow<'a, str>> = Vec::new();
        let mut in_array_of_tables = false;
        let mut aot_prefix: Option<Vec<Cow<'a, str>>> = None;

        loop {
            self.skip_ws_and_newlines();
            let Some(b) = self.peek() else {
                break;
            };

            if b == b'[' {
                self.pos += 1;
                let is_aot = self.peek() == Some(b'[');
                if is_aot {
                    self.pos += 1;
                }
                self.skip_ws();
                self.parse_dotted_key(&mut key_buf)?;
                self.skip_ws();
                if self.peek() != Some(b']') {
                    return Err(self.err("expected ']' to close table header"));
                }
                self.pos += 1;
                if is_aot {
                    if self.peek() != Some(b']') {
                        return Err(self.err("expected ']]' to close array of tables header"));
                    }
                    self.pos += 1;
                }
                self.expect_line_end()?;

                if target.is_none() {
                    if is_aot {
                        ensure_array_table_entry(root, &key_buf)
                            .map_err(|msg| self.err(msg))?;
                    } else {
                        ensure_table_path(root, &key_buf)
                            .map_err(|msg| self.err(msg))?;
                    }
                } else {
                    if is_aot {
                        in_array_of_tables = true;
                        aot_prefix = Some(key_buf.clone());
                    } else if let Some(ref prefix) = aot_prefix {
                        if key_buf.starts_with(prefix) {
                            in_array_of_tables = true;
                        } else {
                            in_array_of_tables = false;
                            aot_prefix = None;
                        }
                    } else {
                        in_array_of_tables = false;
                    }
                }
                current_table.clear();
                current_table.append(&mut key_buf);
            } else {
                self.parse_dotted_key(&mut key_buf)?;
                self.skip_ws();
                if self.peek() != Some(b'=') {
                    return Err(self.err("expected '=' after key"));
                }
                self.pos += 1;
                self.skip_ws();

                match target.as_mut() {
                    None => {
                        let val = self.parse_value(true)?.expect("value required when building");
                        self.expect_line_end()?;
                        insert_value_path(root, &current_table, &key_buf, val)
                            .map_err(|msg| self.err(msg))?;
                    }
                    Some((tgt, found)) => {
                        self.parse_value_for_lookup(
                            &current_table,
                            &key_buf,
                            in_array_of_tables,
                            tgt,
                            found,
                        )?;
                        self.expect_line_end()?;
                    }
                }
            }
        }

        Ok(())
    }

    fn parse_value_for_lookup(
        &mut self,
        table_prefix: &[Cow<'a, str>],
        key_path: &[Cow<'a, str>],
        in_array_of_tables: bool,
        target: &[&str],
        found: &mut Option<Scalar<'a>>,
    ) -> Result<(), Error> {
        if in_array_of_tables {
            let _ = self.parse_value(false)?;
            return Ok(());
        }

        let full_len = table_prefix.len() + key_path.len();
        let is_prefix = full_len <= target.len()
            && table_prefix
                .iter()
                .chain(key_path.iter())
                .zip(target.iter())
                .all(|(seg, tgt)| seg.as_ref() == *tgt);

        if is_prefix && full_len == target.len() {
            match self.peek() {
                Some(b'[') | Some(b'{') => {
                    let _ = self.parse_value(false)?;
                    *found = None;
                }
                _ => {
                    let scalar = self.parse_scalar(true)?.expect("scalar expected");
                    *found = Some(scalar);
                }
            }
            return Ok(());
        }

        if is_prefix && full_len < target.len() && self.peek() == Some(b'{') {
            self.pos += 1;
            self.skip_ws();
            let mut sub_prefix: Vec<Cow<'a, str>> =
                Vec::with_capacity(table_prefix.len() + key_path.len());
            sub_prefix.extend(table_prefix.iter().cloned());
            sub_prefix.extend(key_path.iter().cloned());
            let mut sub_key = Vec::new();
            if self.peek() != Some(b'}') {
                loop {
                    self.skip_ws();
                    self.parse_dotted_key(&mut sub_key)?;
                    self.skip_ws();
                    if self.peek() != Some(b'=') {
                        return Err(self.err("expected '=' in inline table"));
                    }
                    self.pos += 1;
                    self.skip_ws();
                    self.parse_value_for_lookup(&sub_prefix, &sub_key, false, target, found)?;
                    self.skip_ws();
                    match self.peek() {
                        Some(b',') => {
                            self.pos += 1;
                            self.skip_ws();
                        }
                        Some(b'}') => break,
                        _ => return Err(self.err("expected ',' or '}' in inline table")),
                    }
                }
            }
            self.pos += 1;
            return Ok(());
        }

        let _ = self.parse_value(false)?;
        Ok(())
    }

    fn parse_dotted_key(&mut self, out: &mut Vec<Cow<'a, str>>) -> Result<(), Error> {
        out.clear();
        out.push(self.parse_key_segment()?);
        loop {
            self.skip_ws();
            if self.peek() == Some(b'.') {
                self.pos += 1;
                self.skip_ws();
                out.push(self.parse_key_segment()?);
            } else {
                break;
            }
        }
        Ok(())
    }

    fn parse_key_segment(&mut self) -> Result<Cow<'a, str>, Error> {
        match self.peek() {
            Some(b'"') => self.parse_basic_string(true).map(|s| s.unwrap()),
            Some(b'\'') => self.parse_literal_string(true).map(|s| s.unwrap()),
            Some(b) if is_bare_key_byte(b) => {
                let start = self.pos;
                while let Some(ch) = self.peek() {
                    if is_bare_key_byte(ch) {
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                Ok(Cow::Borrowed(&self.input[start..self.pos]))
            }
            _ => Err(self.err("expected key")),
        }
    }

    fn parse_value(&mut self, build: bool) -> Result<Option<Value>, Error> {
        match self.peek() {
            Some(b'[') => {
                self.pos += 1;
                let mut arr = if build { Some(Vec::new()) } else { None };
                loop {
                    self.skip_ws_and_newlines();
                    if self.peek() == Some(b']') {
                        self.pos += 1;
                        break;
                    }
                    let elem = self.parse_value(build)?;
                    if let (Some(vec), Some(val)) = (arr.as_mut(), elem) {
                        vec.push(val);
                    }
                    self.skip_ws_and_newlines();
                    match self.peek() {
                        Some(b',') => {
                            self.pos += 1;
                            self.skip_ws_and_newlines();
                            if self.peek() == Some(b']') {
                                self.pos += 1;
                                break;
                            }
                        }
                        Some(b']') => {
                            self.pos += 1;
                            break;
                        }
                        _ => return Err(self.err("expected ',' or ']' in array")),
                    }
                }
                Ok(arr.map(Value::Array))
            }
            Some(b'{') => {
                self.pos += 1;
                let mut tbl = if build { Some(Table::new()) } else { None };
                let mut key_path = Vec::new();
                self.skip_ws();
                if self.peek() == Some(b'}') {
                    self.pos += 1;
                    return Ok(tbl.map(Value::Table));
                }
                loop {
                    self.skip_ws();
                    self.parse_dotted_key(&mut key_path)?;
                    self.skip_ws();
                    if self.peek() != Some(b'=') {
                        return Err(self.err("expected '=' in inline table"));
                    }
                    self.pos += 1;
                    self.skip_ws();
                    let val = self.parse_value(build)?;
                    if let (Some(table), Some(v)) = (tbl.as_mut(), val) {
                        insert_value_path(table, &[], &key_path, v)
                            .map_err(|msg| self.err(msg))?;
                    }
                    self.skip_ws();
                    match self.peek() {
                        Some(b',') => {
                            self.pos += 1;
                            self.skip_ws();
                            if self.peek() == Some(b'}') {
                                return Err(self.err("trailing comma is not allowed in inline table"));
                            }
                        }
                        Some(b'}') => {
                            self.pos += 1;
                            break;
                        }
                        _ => return Err(self.err("expected ',' or '}' in inline table")),
                    }
                }
                Ok(tbl.map(Value::Table))
            }
            _ => Ok(self.parse_scalar(build)?.map(Scalar::into_value)),
        }
    }

    fn parse_scalar(&mut self, build: bool) -> Result<Option<Scalar<'a>>, Error> {
        if self.starts_with(b"\"\"\"") {
            return Ok(self.parse_multiline_basic_string(build)?.map(Scalar::String));
        }
        if self.starts_with(b"'''") {
            return Ok(self.parse_multiline_literal_string(build)?.map(Scalar::String));
        }
        match self.peek() {
            Some(b'"') => Ok(self.parse_basic_string(build)?.map(Scalar::String)),
            Some(b'\'') => Ok(self.parse_literal_string(build)?.map(Scalar::String)),
            Some(_) => self.parse_bare_scalar(build),
            None => Err(self.err("unexpected end of input while expecting value")),
        }
    }

    fn parse_basic_string(&mut self, build: bool) -> Result<Option<Cow<'a, str>>, Error> {
        self.pos += 1; // opening '"'
        let start = self.pos;
        let mut owned: Option<String> = None;

        while let Some(b) = self.peek() {
            if b == b'"' {
                let end = self.pos;
                self.pos += 1;
                if !build {
                    return Ok(None);
                }
                return Ok(Some(match owned {
                    Some(buf) => Cow::Owned(buf),
                    None => Cow::Borrowed(&self.input[start..end]),
                }));
            }
            if b == b'\n' || b == b'\r' {
                return Err(self.err("unescaped newline in basic string"));
            }
            if b == b'\\' {
                if build && owned.is_none() {
                    owned = Some(self.input[start..self.pos].to_owned());
                }
                self.pos += 1;
                let ch = self.parse_escape_char()?;
                if let Some(buf) = owned.as_mut() {
                    buf.push(ch);
                }
            } else {
                let ch = self.next_utf8_char()?;
                if let Some(buf) = owned.as_mut() {
                    buf.push(ch);
                }
            }
        }
        Err(self.err("unterminated basic string"))
    }

    fn parse_multiline_basic_string(&mut self, build: bool) -> Result<Option<Cow<'a, str>>, Error> {
        self.pos += 3; // opening '"""'
        if self.starts_with(b"\r\n") {
            self.pos += 2;
        } else if self.peek() == Some(b'\n') {
            self.pos += 1;
        }

        let start = self.pos;
        let mut owned: Option<String> = None;

        while self.pos < self.bytes.len() {
            if self.starts_with(b"\"\"\"") {
                let mut quote_count = 0usize;
                while self.bytes.get(self.pos + quote_count) == Some(&b'"') {
                    quote_count += 1;
                }
                if quote_count > 5 {
                    return Err(self.err("too many consecutive quotes in multiline basic string"));
                }
                let extra = quote_count - 3;
                let slice_end = self.pos + extra;
                self.pos += quote_count;
                if !build {
                    return Ok(None);
                }
                return Ok(Some(match owned {
                    Some(mut buf) => {
                        for _ in 0..extra {
                            buf.push('"');
                        }
                        Cow::Owned(buf)
                    }
                    None => Cow::Borrowed(&self.input[start..slice_end]),
                }));
            }

            if self.peek() == Some(b'\\') {
                if build && owned.is_none() {
                    owned = Some(self.input[start..self.pos].to_owned());
                }
                self.pos += 1;
                // Check line-ending backslash
                let save = self.pos;
                self.skip_ws();
                if matches!(self.peek(), Some(b'\n') | Some(b'\r')) {
                    self.skip_ws_and_newlines_no_comments();
                    continue;
                }
                self.pos = save;
                let ch = self.parse_escape_char()?;
                if let Some(buf) = owned.as_mut() {
                    buf.push(ch);
                }
            } else {
                let ch = self.next_utf8_char()?;
                if let Some(buf) = owned.as_mut() {
                    buf.push(ch);
                }
            }
        }
        Err(self.err("unterminated multiline basic string"))
    }

    fn skip_ws_and_newlines_no_comments(&mut self) {
        while let Some(b) = self.peek() {
            if matches!(b, b' ' | b'\t' | b'\n' | b'\r') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn parse_literal_string(&mut self, build: bool) -> Result<Option<Cow<'a, str>>, Error> {
        self.pos += 1; // opening '\''
        let start = self.pos;
        while let Some(b) = self.peek() {
            if b == b'\'' {
                let end = self.pos;
                self.pos += 1;
                return Ok(if build {
                    Some(Cow::Borrowed(&self.input[start..end]))
                } else {
                    None
                });
            }
            if b == b'\n' || b == b'\r' {
                return Err(self.err("newline not allowed in single-line literal string"));
            }
            self.pos += 1;
        }
        Err(self.err("unterminated literal string"))
    }

    fn parse_multiline_literal_string(&mut self, build: bool) -> Result<Option<Cow<'a, str>>, Error> {
        self.pos += 3; // opening '''
        if self.starts_with(b"\r\n") {
            self.pos += 2;
        } else if self.peek() == Some(b'\n') {
            self.pos += 1;
        }
        let start = self.pos;
        while self.pos < self.bytes.len() {
            if self.starts_with(b"'''") {
                let mut quote_count = 0usize;
                while self.bytes.get(self.pos + quote_count) == Some(&b'\'') {
                    quote_count += 1;
                }
                if quote_count > 5 {
                    return Err(self.err("too many consecutive quotes in multiline literal string"));
                }
                let extra = quote_count - 3;
                let end = self.pos + extra;
                self.pos += quote_count;
                return Ok(if build {
                    Some(Cow::Borrowed(&self.input[start..end]))
                } else {
                    None
                });
            }
            self.pos += 1;
        }
        Err(self.err("unterminated multiline literal string"))
    }

    fn parse_escape_char(&mut self) -> Result<char, Error> {
        let Some(esc) = self.peek() else {
            return Err(self.err("unexpected end of string escape"));
        };
        self.pos += 1;
        match esc {
            b'b' => Ok('\u{0008}'),
            b't' => Ok('\t'),
            b'n' => Ok('\n'),
            b'f' => Ok('\u{000C}'),
            b'r' => Ok('\r'),
            b'"' => Ok('"'),
            b'\\' => Ok('\\'),
            b'u' => self.parse_hex_unicode(4),
            b'U' => self.parse_hex_unicode(8),
            _ => Err(self.err("invalid escape sequence")),
        }
    }

    fn parse_hex_unicode(&mut self, digits: usize) -> Result<char, Error> {
        if self.pos + digits > self.bytes.len() {
            return Err(self.err("incomplete unicode escape"));
        }
        let hex = &self.input[self.pos..self.pos + digits];
        self.pos += digits;
        let code = u32::from_str_radix(hex, 16)
            .map_err(|_| self.err("invalid hex digits in unicode escape"))?;
        char::from_u32(code).ok_or_else(|| self.err("invalid unicode scalar value"))
    }

    #[inline]
    fn next_utf8_char(&mut self) -> Result<char, Error> {
        let ch = self.input[self.pos..]
            .chars()
            .next()
            .ok_or_else(|| self.err("unexpected EOF"))?;
        self.pos += ch.len_utf8();
        Ok(ch)
    }

    fn parse_bare_scalar(&mut self, build: bool) -> Result<Option<Scalar<'a>>, Error> {
        let start = self.pos;
        while let Some(b) = self.peek() {
            if is_value_terminator(b) {
                break;
            }
            self.pos += 1;
        }
        let mut token = &self.input[start..self.pos];
        if token.is_empty() {
            return Err(self.err("expected value"));
        }

        // Check space-separated datetime (`YYYY-MM-DD HH:MM:SS...`)
        if is_date_prefix(token.as_bytes()) && self.peek() == Some(b' ') {
            let after_space = self.pos + 1;
            if after_space + 5 <= self.bytes.len()
                && self.bytes[after_space].is_ascii_digit()
                && self.bytes[after_space + 1].is_ascii_digit()
                && self.bytes[after_space + 2] == b':'
            {
                self.pos = after_space;
                while let Some(b) = self.peek() {
                    if is_value_terminator(b) {
                        break;
                    }
                    self.pos += 1;
                }
                token = &self.input[start..self.pos];
                return Ok(if build {
                    Some(Scalar::Datetime(token))
                } else {
                    None
                });
            }
        }

        match token {
            "true" => return Ok(if build { Some(Scalar::Boolean(true)) } else { None }),
            "false" => return Ok(if build { Some(Scalar::Boolean(false)) } else { None }),
            "inf" | "+inf" => {
                return Ok(if build {
                    Some(Scalar::Float(f64::INFINITY))
                } else {
                    None
                });
            }
            "-inf" => {
                return Ok(if build {
                    Some(Scalar::Float(f64::NEG_INFINITY))
                } else {
                    None
                });
            }
            "nan" | "+nan" | "-nan" => {
                return Ok(if build {
                    Some(Scalar::Float(f64::NAN))
                } else {
                    None
                });
            }
            _ => {}
        }

        if looks_like_datetime(token) {
            return Ok(if build {
                Some(Scalar::Datetime(token))
            } else {
                None
            });
        }

        if let Some(int_val) = self.parse_prefixed_int(token)? {
            return Ok(if build {
                Some(Scalar::Integer(int_val))
            } else {
                None
            });
        }

        self.parse_decimal_number(token, build)
    }

    fn parse_prefixed_int(&self, token: &str) -> Result<Option<i64>, Error> {
        let (radix, body) = if let Some(rest) = token.strip_prefix("0x") {
            (16, rest)
        } else if let Some(rest) = token.strip_prefix("0o") {
            (8, rest)
        } else if let Some(rest) = token.strip_prefix("0b") {
            (2, rest)
        } else {
            return Ok(None);
        };

        let mut buf = [0u8; 128];
        let mut len = 0usize;
        let bytes = body.as_bytes();
        if bytes.is_empty() || bytes.first() == Some(&b'_') || bytes.last() == Some(&b'_') {
            return Err(self.err("invalid integer literal"));
        }
        let mut prev_underscore = false;
        for &b in bytes {
            if b == b'_' {
                if prev_underscore {
                    return Err(self.err("consecutive underscores in integer literal"));
                }
                prev_underscore = true;
                continue;
            }
            prev_underscore = false;
            let valid = match radix {
                16 => b.is_ascii_hexdigit(),
                8 => matches!(b, b'0'..=b'7'),
                2 => b == b'0' || b == b'1',
                _ => false,
            };
            if !valid || len >= buf.len() {
                return Err(self.err("invalid integer literal"));
            }
            buf[len] = b;
            len += 1;
        }
        let clean = std::str::from_utf8(&buf[..len]).map_err(|_| self.err("invalid utf8"))?;
        let val = i64::from_str_radix(clean, radix)
            .map_err(|_| self.err("integer literal out of range"))?;
        Ok(Some(val))
    }

    fn parse_decimal_number(&self, token: &str, build: bool) -> Result<Option<Scalar<'a>>, Error> {
        let bytes = token.as_bytes();
        let is_float = bytes.iter().any(|&b| b == b'.' || b == b'e' || b == b'E');

        let mut stack_buf = [0u8; 128];
        let mut heap_buf: Option<Vec<u8>> = None;
        let mut len = 0usize;

        let mut idx = 0usize;
        if matches!(bytes.first(), Some(b'+') | Some(b'-')) {
            stack_buf[0] = bytes[0];
            len = 1;
            idx = 1;
        }
        if idx >= bytes.len() {
            return Err(self.err("invalid numeric literal"));
        }
        if bytes[idx] == b'_' || bytes.last() == Some(&b'_') {
            return Err(self.err("invalid underscore placement in number"));
        }

        // Check leading zeros on integer part
        let int_start = idx;
        if bytes[int_start] == b'0' && int_start + 1 < bytes.len() {
            let next = bytes[int_start + 1];
            if next != b'.' && next != b'e' && next != b'E' {
                return Err(self.err("leading zeros are not allowed in decimal numbers"));
            }
        }

        let mut prev_underscore = false;
        let mut prev_digit = false;
        let mut seen_dot = false;
        let mut seen_exp = false;
        while idx < bytes.len() {
            let b = bytes[idx];
            if b == b'_' {
                if prev_underscore || !prev_digit {
                    return Err(self.err("invalid underscore placement in number"));
                }
                let next = bytes.get(idx + 1).copied();
                if !next.is_some_and(|n| n.is_ascii_digit()) {
                    return Err(self.err("underscore must be followed by a digit"));
                }
                prev_underscore = true;
                prev_digit = false;
                idx += 1;
                continue;
            }
            prev_underscore = false;

            if b.is_ascii_digit() {
                prev_digit = true;
            } else if b == b'.' {
                if seen_dot || seen_exp || !prev_digit || !bytes.get(idx + 1).is_some_and(|n| n.is_ascii_digit()) {
                    return Err(self.err("invalid decimal point in float"));
                }
                seen_dot = true;
                prev_digit = false;
            } else if b == b'e' || b == b'E' {
                if seen_exp || !prev_digit {
                    return Err(self.err("invalid exponent in float"));
                }
                seen_exp = true;
                prev_digit = false;
                if matches!(bytes.get(idx + 1), Some(b'+') | Some(b'-')) {
                    push_byte(&mut stack_buf, &mut len, &mut heap_buf, b);
                    idx += 1;
                    let exp_sign = bytes[idx];
                    push_byte(&mut stack_buf, &mut len, &mut heap_buf, exp_sign);
                    idx += 1;
                    if !bytes.get(idx).is_some_and(|n| n.is_ascii_digit()) {
                        return Err(self.err("expected exponent digits"));
                    }
                    continue;
                } else if !bytes.get(idx + 1).is_some_and(|n| n.is_ascii_digit()) {
                    return Err(self.err("expected exponent digits"));
                }
            } else {
                return Err(self.err(format!("unexpected token '{}'", token)));
            }

            push_byte(&mut stack_buf, &mut len, &mut heap_buf, b);
            idx += 1;
        }

        let clean_bytes = match heap_buf.as_ref() {
            Some(v) => v.as_slice(),
            None => &stack_buf[..len],
        };
        let clean = std::str::from_utf8(clean_bytes).map_err(|_| self.err("invalid utf8"))?;

        if is_float {
            let f = clean
                .parse::<f64>()
                .map_err(|_| self.err("invalid float literal"))?;
            Ok(if build { Some(Scalar::Float(f)) } else { None })
        } else {
            let i = clean
                .parse::<i64>()
                .map_err(|_| self.err("invalid integer literal"))?;
            Ok(if build { Some(Scalar::Integer(i)) } else { None })
        }
    }
}

#[inline]
fn push_byte(stack: &mut [u8; 128], len: &mut usize, heap: &mut Option<Vec<u8>>, b: u8) {
    if let Some(v) = heap.as_mut() {
        v.push(b);
    } else if *len < stack.len() {
        stack[*len] = b;
        *len += 1;
    } else {
        let mut v = Vec::with_capacity(256);
        v.extend_from_slice(&stack[..*len]);
        v.push(b);
        *heap = Some(v);
    }
}

#[inline]
fn is_bare_key_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

#[inline]
fn is_value_terminator(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'#' | b',' | b']' | b'}')
}

fn is_date_prefix(bytes: &[u8]) -> bool {
    bytes.len() == 10
        && bytes[0..4].iter().all(|b| b.is_ascii_digit())
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(|b| b.is_ascii_digit())
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(|b| b.is_ascii_digit())
}

fn looks_like_datetime(token: &str) -> bool {
    let b = token.as_bytes();
    let has_time = b.len() >= 8
        && b[0].is_ascii_digit()
        && b[1].is_ascii_digit()
        && b[2] == b':'
        && b[3].is_ascii_digit()
        && b[4].is_ascii_digit()
        && b[5] == b':';
    let has_date = b.len() >= 10
        && b[0..4].iter().all(|c| c.is_ascii_digit())
        && b[4] == b'-'
        && b[5..7].iter().all(|c| c.is_ascii_digit())
        && b[7] == b'-'
        && b[8..10].iter().all(|c| c.is_ascii_digit());
    has_date || has_time
}

fn ensure_table_path<'a>(mut table: &'a mut Table, path: &[Cow<'_, str>]) -> Result<&'a mut Table, String> {
    for seg in path {
        let key = seg.as_ref();
        if !table.contains_key(key) {
            table.insert(key.to_owned(), Value::Table(Table::new()));
        }
        let entry = table.get_mut(key).unwrap();
        match entry {
            Value::Table(sub) => table = sub,
            Value::Array(arr) => {
                let Some(Value::Table(sub)) = arr.last_mut() else {
                    return Err(format!("cannot redefine key '{}' as table", key));
                };
                table = sub;
            }
            _ => return Err(format!("cannot redefine key '{}' as table", key)),
        }
    }
    Ok(table)
}

fn ensure_array_table_entry(mut table: &mut Table, path: &[Cow<'_, str>]) -> Result<(), String> {
    if path.is_empty() {
        return Err("empty array-of-tables header".to_owned());
    }
    if path.len() > 1 {
        table = ensure_table_path(table, &path[..path.len() - 1])?;
    }
    let last_key = path.last().unwrap().as_ref();
    if !table.contains_key(last_key) {
        table.insert(last_key.to_owned(), Value::Array(Vec::new()));
    }
    let Some(Value::Array(arr)) = table.get_mut(last_key) else {
        return Err(format!("cannot redefine '{}' as array of tables", last_key));
    };
    arr.push(Value::Table(Table::new()));
    Ok(())
}

fn insert_value_path(
    root: &mut Table,
    table_prefix: &[Cow<'_, str>],
    key_path: &[Cow<'_, str>],
    value: Value,
) -> Result<(), String> {
    if key_path.is_empty() {
        return Err("empty key path".to_owned());
    }
    let mut target = ensure_table_path(root, table_prefix)?;
    if key_path.len() > 1 {
        target = ensure_table_path(target, &key_path[..key_path.len() - 1])?;
    }
    let last_key = key_path.last().unwrap().as_ref();
    if target.contains_key(last_key) {
        return Err(format!("duplicate key '{}'", last_key));
    }
    target.insert(last_key.to_owned(), value);
    Ok(())
}
