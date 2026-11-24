// Semantic serializer streaming Eronom's tagged Value directly into JSON text buffers.

use std::fmt::Write;
use crate::gc::{GcData, GcObject};
use crate::value::Value;
use super::wire::append_quote;

/// Serializes an Eronom `Value` into the destination string buffer.
/// If `indent` is provided, the output is pretty-printed with that number of spaces.
pub fn stringify_value(val: Value, out: &mut String, indent: Option<usize>) -> Result<(), &'static str> {
    let mut visited: Vec<*mut GcObject> = Vec::new();
    stringify_inner(val, out, indent, 0, &mut visited)
}

fn stringify_inner(
    val: Value,
    out: &mut String,
    indent: Option<usize>,
    depth: usize,
    visited: &mut Vec<*mut GcObject>,
) -> Result<(), &'static str> {
    if val.is_null() {
        out.push_str("null");
    } else if val.is_boolean() {
        out.push_str(if val.as_boolean() { "true" } else { "false" });
    } else if val.is_number() {
        let n = val.as_number();
        if n.is_finite() {
            if n.fract() == 0.0 && n.abs() <= 9007199254740991.0 {
                let _ = write!(out, "{}", n as i64);
            } else {
                let _ = write!(out, "{}", n);
            }
        } else {
            // In JSON specification, NaN and Infinities are serialized as null
            out.push_str("null");
        }
    } else if let Some(s) = val.as_str() {
        append_quote(out, s);
    } else if val.is_array() {
        unsafe {
            let ptr = val.as_gc_ptr();
            if visited.contains(&ptr) {
                return Err("TypeError: Converting circular structure to JSON");
            }
            visited.push(ptr);

            if let GcData::Array(ref arr) = (*ptr).data {
                if arr.is_empty() {
                    out.push_str("[]");
                } else {
                    out.push('[');
                    for (i, &item) in arr.iter().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        if let Some(spaces) = indent {
                            out.push('\n');
                            write_indent(out, (depth + 1) * spaces);
                        }
                        stringify_inner(item, out, indent, depth + 1, visited)?;
                    }
                    if let Some(spaces) = indent {
                        out.push('\n');
                        write_indent(out, depth * spaces);
                    }
                    out.push(']');
                }
            } else {
                out.push_str("[]");
            }

            visited.pop();
        }
    } else if val.is_object() {
        unsafe {
            let ptr = val.as_gc_ptr();
            if visited.contains(&ptr) {
                return Err("TypeError: Converting circular structure to JSON");
            }
            visited.push(ptr);

            match (*ptr).data {
                GcData::Struct(ref s) => {
                    if s.fields.is_empty() {
                        out.push_str("{}");
                    } else {
                        out.push('{');
                        // Collect fields in declaration order (0, 1, 2...)
                        let mut ordered_fields = vec![("", Value::null()); s.fields.len()];
                        for (map_key, &idx) in &s.descriptor.field_indices {
                            if idx < ordered_fields.len() {
                                ordered_fields[idx] = (map_key.0.as_str().unwrap_or(""), s.fields[idx]);
                            }
                        }

                        let mut first = true;
                        for (name, field_val) in ordered_fields {
                            if !first {
                                out.push(',');
                            }
                            first = false;

                            if let Some(spaces) = indent {
                                out.push('\n');
                                write_indent(out, (depth + 1) * spaces);
                            }

                            append_quote(out, name);
                            out.push(':');
                            if indent.is_some() {
                                out.push(' ');
                            }

                            stringify_inner(field_val, out, indent, depth + 1, visited)?;
                        }
                        if let Some(spaces) = indent {
                            out.push('\n');
                            write_indent(out, depth * spaces);
                        }
                        out.push('}');
                    }
                }
                GcData::Object(ref map) => {
                    if map.is_empty() {
                        out.push_str("{}");
                    } else {
                        out.push('{');
                        let mut first = true;
                        for (k, &v) in map {
                            if let Some(key_str) = k.0.as_str() {
                                if !first {
                                    out.push(',');
                                }
                                first = false;

                                if let Some(spaces) = indent {
                                    out.push('\n');
                                    write_indent(out, (depth + 1) * spaces);
                                }

                                append_quote(out, key_str);
                                out.push(':');
                                if indent.is_some() {
                                    out.push(' ');
                                }

                                stringify_inner(v, out, indent, depth + 1, visited)?;
                            }
                        }
                        if let Some(spaces) = indent {
                            out.push('\n');
                            write_indent(out, depth * spaces);
                        }
                        out.push('}');
                    }
                }
                _ => out.push_str("{}"),
            }

            visited.pop();
        }
    } else {
        out.push_str("null");
    }

    Ok(())
}

#[inline(always)]
fn write_indent(out: &mut String, count: usize) {
    for _ in 0..count {
        out.push(' ');
    }
}
