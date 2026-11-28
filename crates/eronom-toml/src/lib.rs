//! Zero-dependency in-tree TOML parser and scalar lookup engine for Eronom.

mod error;
mod parser;
mod value;

use std::borrow::Cow;

pub use error::Error;
pub use value::{Table, Value};

/// Parses a TOML document string into a root [`Value::Table`].
#[inline]
pub fn from_str(input: &str) -> Result<Value, Error> {
    Ok(Value::Table(parser::parse_document(input)?))
}

/// Parses a TOML document string into a root [`Table`].
#[inline]
pub fn parse_table(input: &str) -> Result<Table, Error> {
    parser::parse_document(input)
}

/// Scans a TOML document for a single integer key path without building a full DOM.
#[inline]
pub fn lookup_integer(input: &str, path: &[&str]) -> Option<i64> {
    match parser::lookup_scalar(input, path).ok().flatten()? {
        parser::Scalar::Integer(i) => Some(i),
        _ => None,
    }
}

/// Scans a TOML document for a single boolean key path without building a full DOM.
#[inline]
pub fn lookup_bool(input: &str, path: &[&str]) -> Option<bool> {
    match parser::lookup_scalar(input, path).ok().flatten()? {
        parser::Scalar::Boolean(b) => Some(b),
        _ => None,
    }
}

/// Scans a TOML document for a single string key path without building a full DOM.
#[inline]
pub fn lookup_str<'a>(input: &'a str, path: &[&str]) -> Option<Cow<'a, str>> {
    match parser::lookup_scalar(input, path).ok().flatten()? {
        parser::Scalar::String(s) => Some(s),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_eronom_config_and_fast_lookups() {
        let src = r#"
        # Eronom configuration
        [server]
        port = 5000
        host = "0.0.0.0"
        dev = "todos.er"

        [eds]
        enabled = false
        "#;

        assert_eq!(lookup_integer(src, &["server", "port"]), Some(5000));
        assert_eq!(lookup_str(src, &["server", "dev"]).as_deref(), Some("todos.er"));
        assert_eq!(lookup_bool(src, &["eds", "enabled"]), Some(false));
        assert_eq!(lookup_integer(src, &["server", "missing"]), None);

        let val = from_str(src).unwrap();
        assert_eq!(val.get("server").and_then(|s| s.get("port")).and_then(|v| v.as_integer()), Some(5000));
        assert_eq!(val.get("server").and_then(|s| s.get("dev")).and_then(|v| v.as_str()), Some("todos.er"));
        assert_eq!(val.get("eds").and_then(|s| s.get("enabled")).and_then(|v| v.as_bool()), Some(false));
    }

    #[test]
    fn supports_dotted_keys_and_inline_tables_in_both_modes() {
        let src = r#"
        server.port = 8080
        eds = { enabled = true, theme = 'dark' }
        "#;

        assert_eq!(lookup_integer(src, &["server", "port"]), Some(8080));
        assert_eq!(lookup_bool(src, &["eds", "enabled"]), Some(true));
        assert_eq!(lookup_str(src, &["eds", "theme"]).as_deref(), Some("dark"));

        let val = from_str(src).unwrap();
        assert_eq!(val.get("server").and_then(|s| s.get("port")).and_then(|v| v.as_integer()), Some(8080));
        assert_eq!(val.get("eds").and_then(|e| e.get("theme")).and_then(|v| v.as_str()), Some("dark"));
    }

    #[test]
    fn supports_strings_numbers_arrays_and_aot() {
        let src = r#"
        title = "hello\n\"world\" \u26A1"
        raw = 'no\escapes'
        multi = """
first line
second "quoted" line"""
        multi_raw = '''
literal \ line'''
        dec = 1_000_000
        neg = -42
        hex = 0xff_ff
        oct = 0o755
        bin = 0b1010_0101
        flt = 3.14_15
        exp = 1e-3
        inf_val = inf
        created = 2026-11-28T14:30:00Z
        local_dt = 2026-11-28 14:30:00
        ports = [
            3000, # primary
            3001, # trailing comma ok in arrays
        ]

        [[routes]]
        path = "/a"

        [[routes]]
        path = "/b"
        "#;

        let val = from_str(src).unwrap();
        assert_eq!(val.get("title").and_then(|v| v.as_str()), Some("hello\n\"world\" ⚡"));
        assert_eq!(val.get("raw").and_then(|v| v.as_str()), Some("no\\escapes"));
        assert_eq!(val.get("multi").and_then(|v| v.as_str()), Some("first line\nsecond \"quoted\" line"));
        assert_eq!(val.get("multi_raw").and_then(|v| v.as_str()), Some("literal \\ line"));
        assert_eq!(val.get("dec").and_then(|v| v.as_integer()), Some(1_000_000));
        assert_eq!(val.get("neg").and_then(|v| v.as_integer()), Some(-42));
        assert_eq!(val.get("hex").and_then(|v| v.as_integer()), Some(0xffff));
        assert_eq!(val.get("oct").and_then(|v| v.as_integer()), Some(0o755));
        assert_eq!(val.get("bin").and_then(|v| v.as_integer()), Some(0b1010_0101));
        assert_eq!(val.get("flt").and_then(|v| v.as_float()), Some(3.1415));
        assert_eq!(val.get("exp").and_then(|v| v.as_float()), Some(1e-3));
        assert!(val.get("inf_val").and_then(|v| v.as_float()).unwrap().is_infinite());
        assert_eq!(val.get("ports").and_then(|v| v.as_array()).map(|a| a.len()), Some(2));
        let routes = val.get("routes").and_then(|v| v.as_array()).unwrap();
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].get("path").and_then(|v| v.as_str()), Some("/a"));
        assert_eq!(routes[1].get("path").and_then(|v| v.as_str()), Some("/b"));
    }

    #[test]
    fn rejects_invalid_toml_cleanly() {
        assert!(from_str("[unclosed").is_err());
        assert!(from_str("a = 01").is_err());
        assert!(from_str("a = 1__2").is_err());
        assert!(from_str("a = \"unterminated").is_err());
        assert!(from_str("a = 1\na = 2").is_err());
        assert_eq!(lookup_integer("port = 3000\n[broken", &["port"]), None);
    }

    #[test]
    fn lookup_ignores_array_of_tables_and_nested_subtables() {
        let src = r#"
        [[items]]
        [items.meta]
        port = 9999

        [server]
        port = 4000
        "#;
        assert_eq!(lookup_integer(src, &["items", "meta", "port"]), None);
        assert_eq!(lookup_integer(src, &["server", "port"]), Some(4000));
    }
}
