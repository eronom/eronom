// Copyright 2024 Eronom Project. All rights reserved.
// High-performance, zero-AST streaming JSON engine for Eronom runtime.

pub mod wire;
pub mod syntax;
pub mod de;
pub mod ser;
#[cfg(test)]
pub mod tests;

pub use wire::append_quote as escape_json_string;
pub use de::parse_json;
pub use ser::stringify_value;

/// Parses JSON text directly into an Eronom `Value`.
#[inline]
pub fn json_parse(input: &str) -> Option<crate::value::Value> {
    de::parse_json(input).ok()
}

/// Stringifies an Eronom `Value` into a JSON string.
#[inline]
pub fn json_stringify(val: crate::value::Value, indent: Option<usize>) -> String {
    let mut out = String::with_capacity(128);
    let _ = ser::stringify_value(val, &mut out, indent);
    out
}

