// Copyright 2024 Eronom Project. All rights reserved.
// Stateless wire format processing for JSON text (ported from jsonwire).

/// Lookup table indicating which ASCII characters require escaping in JSON.
/// Characters < 0x20 (control characters), '"' (0x22), and '\\' (0x5c) require escaping.
pub const ESCAPE_ASCII: [bool; 256] = {
    let mut table = [false; 256];
    let mut i = 0;
    while i < 0x20 {
        table[i] = true;
        i += 1;
    }
    table[b'"' as usize] = true;
    table[b'\\' as usize] = true;
    table
};

/// Consumes leading whitespace according to RFC 8259: space (0x20), tab (0x09), line feed (0x0a), carriage return (0x0d).
#[inline(always)]
pub fn consume_whitespace(b: &[u8]) -> usize {
    let mut n = 0;
    while n < b.len() {
        match b[n] {
            b' ' | b'\t' | b'\n' | b'\r' => n += 1,
            _ => break,
        }
    }
    n
}

/// Consumes the literal "null" from input. Returns 4 if matched, otherwise 0.
#[inline(always)]
pub fn consume_null(b: &[u8]) -> usize {
    const LITERAL: &[u8] = b"null";
    if b.len() >= LITERAL.len() && &b[..LITERAL.len()] == LITERAL {
        LITERAL.len()
    } else {
        0
    }
}

/// Consumes the literal "true" or "false". Returns Some((bool, bytes_consumed)) or None.
#[inline(always)]
pub fn consume_bool(b: &[u8]) -> Option<(bool, usize)> {
    if b.len() >= 4 && &b[..4] == b"true" {
        Some((true, 4))
    } else if b.len() >= 5 && &b[..5] == b"false" {
        Some((false, 5))
    } else {
        None
    }
}

/// Strictly validates and consumes an RFC 8259 number from the start of b.
/// Returns the number of bytes consumed if valid, or an error description.
#[inline]
pub fn consume_number(b: &[u8]) -> Result<usize, &'static str> {
    if b.is_empty() {
        return Err("Unexpected EOF while parsing number");
    }

    let mut i = 0;
    if b[i] == b'-' {
        i += 1;
        if i >= b.len() {
            return Err("Invalid number: trailing minus sign");
        }
    }

    // Integer component
    if b[i] == b'0' {
        i += 1;
        if i < b.len() && b[i].is_ascii_digit() {
            return Err("Invalid number: leading zero is forbidden");
        }
    } else if b[i].is_ascii_digit() {
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    } else {
        return Err("Invalid number: expected digit");
    }

    // Fractional component
    if i < b.len() && b[i] == b'.' {
        i += 1;
        if i >= b.len() || !b[i].is_ascii_digit() {
            return Err("Invalid number: expected digit after decimal point");
        }
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }

    // Exponent component
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        if i >= b.len() || !b[i].is_ascii_digit() {
            return Err("Invalid number: expected digit in exponent");
        }
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }

    Ok(i)
}

/// Appends a double-quoted, escaped JSON string representation of `src` to `dst`.
/// Escapes control characters, quotes, and backslashes per RFC 8259.
pub fn append_quote(dst: &mut String, src: &str) {
    dst.reserve(src.len() + 2);
    dst.push('"');

    let bytes = src.as_bytes();
    let mut start = 0;

    for (i, &b) in bytes.iter().enumerate() {
        if ESCAPE_ASCII[b as usize] {
            if i > start {
                dst.push_str(&src[start..i]);
            }
            match b {
                b'"' => dst.push_str("\\\""),
                b'\\' => dst.push_str("\\\\"),
                0x08 => dst.push_str("\\b"),
                0x0C => dst.push_str("\\f"),
                b'\n' => dst.push_str("\\n"),
                b'\r' => dst.push_str("\\r"),
                b'\t' => dst.push_str("\\t"),
                c => {
                    use std::fmt::Write;
                    let _ = write!(dst, "\\u{:04x}", c);
                }
            }
            start = i + 1;
        }
    }

    if start < bytes.len() {
        dst.push_str(&src[start..]);
    }

    dst.push('"');
}

/// Unquotes a JSON string literal's contents (excluding the outer quotes),
/// decoding escape sequences and UTF-16 surrogate pairs into UTF-8.
pub fn append_unquote(dst: &mut String, src: &[u8]) -> Result<(), &'static str> {
    let mut i = 0;
    while i < src.len() {
        let b = src[i];
        if b < 0x20 {
            return Err("Invalid unescaped control character in string");
        }
        if b == b'\\' {
            i += 1;
            if i >= src.len() {
                return Err("Unterminated escape sequence in string");
            }
            match src[i] {
                b'"' => { dst.push('"'); i += 1; }
                b'\\' => { dst.push('\\'); i += 1; }
                b'/' => { dst.push('/'); i += 1; }
                b'b' => { dst.push('\x08'); i += 1; }
                b'f' => { dst.push('\x0C'); i += 1; }
                b'n' => { dst.push('\n'); i += 1; }
                b'r' => { dst.push('\r'); i += 1; }
                b't' => { dst.push('\t'); i += 1; }
                b'u' => {
                    i += 1;
                    let u1 = decode_hex_u16(src, &mut i)?;
                    // Check for UTF-16 surrogate pair
                    if (0xD800..=0xDBFF).contains(&u1) {
                        // High surrogate - must be followed by \uDC00..\uDFFF
                        if i + 6 <= src.len() && src[i] == b'\\' && src[i + 1] == b'u' {
                            i += 2;
                            let u2 = decode_hex_u16(src, &mut i)?;
                            if (0xDC00..=0xDFFF).contains(&u2) {
                                let code_point = 0x10000 + (((u1 as u32 - 0xD800) << 10) | (u2 as u32 - 0xDC00));
                                if let Some(ch) = char::from_u32(code_point) {
                                    dst.push(ch);
                                } else {
                                    return Err("Invalid Unicode surrogate code point");
                                }
                            } else {
                                return Err("Invalid low surrogate in Unicode escape pair");
                            }
                        } else {
                            return Err("Unpaired high surrogate in Unicode escape");
                        }
                    } else if (0xDC00..=0xDFFF).contains(&u1) {
                        return Err("Unpaired low surrogate in Unicode escape");
                    } else if let Some(ch) = char::from_u32(u1 as u32) {
                        dst.push(ch);
                    } else {
                        return Err("Invalid Unicode escape code point");
                    }
                }
                _ => return Err("Invalid escape character in string"),
            }
        } else {
            // Find contiguous non-escape run
            let start = i;
            while i < src.len() && src[i] != b'\\' && src[i] >= 0x20 {
                i += 1;
            }
            let chunk = std::str::from_utf8(&src[start..i])
                .map_err(|_| "Invalid UTF-8 sequence in string")?;
            dst.push_str(chunk);
        }
    }
    Ok(())
}

#[inline(always)]
fn decode_hex_u16(src: &[u8], i: &mut usize) -> Result<u16, &'static str> {
    if *i + 4 > src.len() {
        return Err("Truncated \\u hex escape");
    }
    let hex_slice = &src[*i..*i + 4];
    *i += 4;
    let s = std::str::from_utf8(hex_slice)
        .map_err(|_| "Non-UTF-8 in hex escape")?;
    u16::from_str_radix(s, 16)
        .map_err(|_| "Invalid hexadecimal digit in \\u escape")
}

