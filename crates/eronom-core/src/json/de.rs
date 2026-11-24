// Semantic deserializer binding streaming JSON tokens directly to Eronom's GC heap.

use crate::gc::{gc_alloc_string, gc_allocate, get_pooled_vec, intern_string, GcData, GcStruct};
use crate::shape::get_or_create_anonymous_shape;
use crate::value::Value;
use super::syntax::{JsonDecoder, Token};

/// Parses an RFC 8259 JSON string directly into an Eronom `Value`.
pub fn parse_json(input: &str) -> Result<Value, &'static str> {
    let mut decoder = JsonDecoder::new(input);
    let val = parse_value(&mut decoder)?;
    if !decoder.is_eof() {
        return Err("Unexpected trailing characters after JSON value");
    }
    Ok(val)
}

/// Recursively parses a single JSON value from the token stream.
pub fn parse_value<'a>(decoder: &mut JsonDecoder<'a>) -> Result<Value, &'static str> {
    match decoder.next_token()? {
        None => Err("Unexpected end of JSON input"),
        Some(Token::Null) => Ok(Value::null()),
        Some(Token::Bool(b)) => Ok(Value::boolean(b)),
        Some(Token::Number(s)) => {
            let num = s.parse::<f64>().map_err(|_| "Failed to parse float")?;
            Ok(Value::number(num))
        }
        Some(Token::String(s)) => {
            let ptr = gc_alloc_string(&s);
            Ok(Value::string(ptr))
        }
        Some(Token::BeginArray) => {
            let mut elements: Vec<Value> = get_pooled_vec(4);
            if let Some(&Token::EndArray) = decoder.peek_token()? {
                decoder.next_token()?; // consume ']'
                let ptr = gc_allocate(GcData::Array(elements));
                return Ok(Value::array(ptr));
            }
            loop {
                let elem = parse_value(decoder)?;
                elements.push(elem);

                match decoder.next_token()? {
                    Some(Token::Comma) => {
                        // Trailing commas are strictly prohibited by RFC 8259
                        if let Some(&Token::EndArray) = decoder.peek_token()? {
                            return Err("Trailing comma is not allowed in JSON array");
                        }
                    }
                    Some(Token::EndArray) => break,
                    _ => return Err("Expected ',' or ']' inside array"),
                }
            }
            let ptr = gc_allocate(GcData::Array(elements));
            Ok(Value::array(ptr))
        }
        Some(Token::BeginObject) => {
            let mut keys: Vec<Value> = get_pooled_vec(4);
            let mut values: Vec<Value> = get_pooled_vec(4);

            if let Some(&Token::EndObject) = decoder.peek_token()? {
                decoder.next_token()?; // consume '}'
                let (desc, _) = get_or_create_anonymous_shape(&[]);
                let ptr = gc_allocate(GcData::Struct(GcStruct {
                    descriptor: desc,
                    fields: get_pooled_vec(0),
                }));
                return Ok(Value::object(ptr));
            }

            loop {
                // Expect property key string
                let key_str = match decoder.next_token()? {
                    Some(Token::String(s)) => s,
                    _ => return Err("Expected string key in object"),
                };

                // Intern string key directly (zero allocation for known keys)
                let key_ptr = intern_string(&key_str);
                keys.push(Value::string(key_ptr));

                // Expect ':'
                match decoder.next_token()? {
                    Some(Token::Colon) => {}
                    _ => return Err("Expected ':' after object key"),
                }

                // Parse property value
                let val = parse_value(decoder)?;
                values.push(val);

                match decoder.next_token()? {
                    Some(Token::Comma) => {
                        // Trailing commas are strictly prohibited by RFC 8259
                        if let Some(&Token::EndObject) = decoder.peek_token()? {
                            return Err("Trailing comma is not allowed in JSON object");
                        }
                    }
                    Some(Token::EndObject) => break,
                    _ => return Err("Expected ',' or '}' inside object"),
                }
            }

            // Retrieve or create Shape descriptor from cached shapes
            let (desc, offsets) = get_or_create_anonymous_shape(&keys);
            let mut fields = get_pooled_vec(keys.len());
            fields.resize(keys.len(), Value::null());
            for i in 0..keys.len() {
                fields[offsets[i]] = values[i];
            }

            let ptr = gc_allocate(GcData::Struct(GcStruct {
                descriptor: desc,
                fields,
            }));
            Ok(Value::object(ptr))
        }
        Some(Token::EndArray) | Some(Token::EndObject) | Some(Token::Comma) | Some(Token::Colon) => {
            Err("Unexpected delimiter token in JSON stream")
        }
    }
}
