// Copyright 2024 Eronom Project. All rights reserved.
// Comprehensive unit tests for the Eronom native JSON engine.

use super::*;
use crate::value::Value;

#[test]
fn test_parse_primitives() {
    let null_val = json_parse("null").expect("parse null");
    assert!(null_val.is_null());

    let true_val = json_parse("true").expect("parse true");
    assert!(true_val.is_boolean());
    assert_eq!(true_val.as_boolean(), true);

    let false_val = json_parse("false").expect("parse false");
    assert!(false_val.is_boolean());
    assert_eq!(false_val.as_boolean(), false);

    let num_val = json_parse("42.5").expect("parse float");
    assert!(num_val.is_number());
    assert_eq!(num_val.as_number(), 42.5);

    let neg_num = json_parse("-100").expect("parse neg int");
    assert_eq!(neg_num.as_number(), -100.0);

    let str_val = json_parse("\"hello world\"").expect("parse string");
    assert_eq!(str_val.as_str(), Some("hello world"));
}

#[test]
fn test_parse_string_escapes_and_surrogates() {
    let escaped = json_parse(r#""quote: \" backslash: \\ newline: \n tab: \t""#).expect("parse escapes");
    assert_eq!(escaped.as_str(), Some("quote: \" backslash: \\ newline: \n tab: \t"));

    // \u0041 = 'A'
    let unicode = json_parse(r#""\u0041BC""#).expect("parse unicode hex");
    assert_eq!(unicode.as_str(), Some("ABC"));

    // UTF-16 surrogate pair for 😀: \uD83D\uDE00
    let emoji = json_parse(r#""\uD83D\uDE00""#).expect("parse surrogate pair");
    assert_eq!(emoji.as_str(), Some("😀"));
}

#[test]
fn test_parse_arrays() {
    let empty = json_parse("[]").expect("parse empty array");
    assert!(empty.is_array());

    let arr = json_parse("[1, 2, 3, true, null, \"test\"]").expect("parse mixed array");
    assert!(arr.is_array());

    let stringified = json_stringify(arr, None);
    assert_eq!(stringified, "[1,2,3,true,null,\"test\"]");
}

#[test]
fn test_parse_objects_and_ordering() {
    let json_text = r#"{"id":101,"name":"Eronom","active":true,"tags":["fast","jit"]}"#;
    let obj = json_parse(json_text).expect("parse object");
    assert!(obj.is_object());

    // Round trip stringify to verify key order and values
    let output = json_stringify(obj, None);
    assert_eq!(output, json_text);
}

#[test]
fn test_invalid_json_rejection() {
    // Trailing comma
    assert!(json_parse("[1, 2,]").is_none());
    assert!(json_parse(r#"{"a": 1,}"#).is_none());

    // Leading zero
    assert!(json_parse("0123").is_none());

    // Explicit plus
    assert!(json_parse("+5").is_none());

    // Unclosed string
    assert!(json_parse(r#""unclosed"#).is_none());

    // Trailing garbage
    assert!(json_parse("null extra").is_none());
}

#[test]
fn test_nesting_depth_limit() {
    // Build nested array of depth 130 (> MAX_DEPTH 128)
    let mut deep = String::new();
    for _ in 0..130 {
        deep.push('[');
    }
    for _ in 0..130 {
        deep.push(']');
    }

    assert!(json_parse(&deep).is_none(), "Should reject nesting depth > 128");
}

#[test]
fn test_pretty_print() {
    let json_text = r#"{"a":1,"b":[2,3]}"#;
    let obj = json_parse(json_text).expect("parse");
    let pretty = json_stringify(obj, Some(2));
    assert!(pretty.contains("\n  \"a\": 1,"));
    assert!(pretty.contains("\n  \"b\": ["));
}

#[test]
fn test_circular_reference_protection() {
    use crate::gc::{gc_allocate, GcData};
    let elements = Vec::new();
    let ptr = gc_allocate(GcData::Array(elements));
    let val = Value::array(ptr);
    unsafe {
        if let GcData::Array(ref mut arr) = (*ptr).data {
            arr.push(val); // self-referencing array
        }
    }
    let mut out = String::new();
    let err = stringify_value(val, &mut out, None);
    assert!(err.is_err());
    assert_eq!(err.unwrap_err(), "TypeError: Converting circular structure to JSON");
}
