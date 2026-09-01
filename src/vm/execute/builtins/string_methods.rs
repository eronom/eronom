use crate::vm::value::Value;
use crate::vm::gc::{gc_alloc_string, gc_alloc_array, BuiltinMethodId};

pub fn get_string_builtin_method_id(name: &str) -> Option<BuiltinMethodId> {
    use BuiltinMethodId::*;
    match name {
        "toUpperCase" => Some(StringToUpperCase),
        "toLowerCase" => Some(StringToLowerCase),
        "trim" => Some(StringTrim),
        "trimStart" | "trimLeft" => Some(StringTrimStart),
        "trimEnd" | "trimRight" => Some(StringTrimEnd),
        "split" => Some(StringSplit),
        "slice" => Some(StringSlice),
        "substring" => Some(StringSubstring),
        "indexOf" => Some(StringIndexOf),
        "lastIndexOf" => Some(StringLastIndexOf),
        "includes" => Some(StringIncludes),
        "startsWith" => Some(StringStartsWith),
        "endsWith" => Some(StringEndsWith),
        "replace" => Some(StringReplace),
        "replaceAll" => Some(StringReplaceAll),
        "charAt" => Some(StringCharAt),
        "charCodeAt" => Some(StringCharCodeAt),
        "repeat" => Some(StringRepeat),
        "padStart" => Some(StringPadStart),
        "padEnd" => Some(StringPadEnd),
        "concat" => Some(StringConcat),
        _ => None,
    }
}

pub fn execute_string_method(
    receiver: Value,
    method: BuiltinMethodId,
    args: &[Value],
) -> Result<Value, String> {
    use BuiltinMethodId::*;
    match method {
        StringToUpperCase => {
            let s = receiver.as_str().unwrap_or("");
            let res = s.to_uppercase();
            let ptr = gc_alloc_string(&res);
            Ok(Value::string(ptr))
        }
        StringToLowerCase => {
            let s = receiver.as_str().unwrap_or("");
            let res = s.to_lowercase();
            let ptr = gc_alloc_string(&res);
            Ok(Value::string(ptr))
        }
        StringTrim => {
            let s = receiver.as_str().unwrap_or("");
            let res = s.trim();
            let ptr = gc_alloc_string(res);
            Ok(Value::string(ptr))
        }
        StringTrimStart => {
            let s = receiver.as_str().unwrap_or("");
            let res = s.trim_start();
            let ptr = gc_alloc_string(res);
            Ok(Value::string(ptr))
        }
        StringTrimEnd => {
            let s = receiver.as_str().unwrap_or("");
            let res = s.trim_end();
            let ptr = gc_alloc_string(res);
            Ok(Value::string(ptr))
        }
        StringSplit => {
            let s = receiver.as_str().unwrap_or("");
            if args.is_empty() {
                let ptr = gc_alloc_array(&[receiver]);
                Ok(Value::array(ptr))
            } else {
                let sep = args[0].as_str().unwrap_or("");
                let limit = args.get(1).and_then(|v| if v.is_number() { Some(v.as_number() as usize) } else { None });
                let parts: Vec<Value> = if sep.is_empty() {
                    let mut p = Vec::new();
                    for c in s.chars() {
                        let s_c = c.to_string();
                        let ptr = gc_alloc_string(&s_c);
                        p.push(Value::string(ptr));
                        if let Some(lim) = limit {
                            if p.len() >= lim { break; }
                        }
                    }
                    p
                } else {
                    let mut p = Vec::new();
                    for part in s.split(sep) {
                        let ptr = gc_alloc_string(part);
                        p.push(Value::string(ptr));
                        if let Some(lim) = limit {
                            if p.len() >= lim { break; }
                        }
                    }
                    p
                };
                let ptr = gc_alloc_array(&parts);
                Ok(Value::array(ptr))
            }
        }
        StringSlice => {
            let s = receiver.as_str().unwrap_or("");
            let chars: Vec<char> = s.chars().collect();
            let len = chars.len() as isize;
            let start = args.get(0).map(|v| if v.is_number() { v.as_number() as isize } else { 0 }).unwrap_or(0);
            let end = args.get(1).map(|v| if v.is_number() { v.as_number() as isize } else { len }).unwrap_or(len);
            let start_idx = if start < 0 { (len + start).max(0) as usize } else { (start as usize).min(chars.len()) };
            let end_idx = if end < 0 { (len + end).max(0) as usize } else { (end as usize).min(chars.len()) };
            if start_idx >= end_idx {
                let ptr = gc_alloc_string("");
                Ok(Value::string(ptr))
            } else {
                let sub: String = chars[start_idx..end_idx].iter().collect();
                let ptr = gc_alloc_string(&sub);
                Ok(Value::string(ptr))
            }
        }
        StringSubstring => {
            let s = receiver.as_str().unwrap_or("");
            let chars: Vec<char> = s.chars().collect();
            let len = chars.len() as isize;
            let mut start = args.get(0).map(|v| if v.is_number() { v.as_number() as isize } else { 0 }).unwrap_or(0).max(0) as usize;
            let mut end = args.get(1).map(|v| if v.is_number() { v.as_number() as isize } else { len }).unwrap_or(len).max(0) as usize;
            start = start.min(chars.len());
            end = end.min(chars.len());
            if start > end {
                std::mem::swap(&mut start, &mut end);
            }
            let sub: String = chars[start..end].iter().collect();
            let ptr = gc_alloc_string(&sub);
            Ok(Value::string(ptr))
        }
        StringIndexOf => {
            let s = receiver.as_str().unwrap_or("");
            let search = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
            let from_idx = args.get(1).map(|v| if v.is_number() { (v.as_number() as usize).min(s.len()) } else { 0 }).unwrap_or(0);
            if from_idx <= s.len() {
                if let Some(pos) = s[from_idx..].find(search) {
                    return Ok(Value::number((from_idx + pos) as f64));
                }
            }
            Ok(Value::number(-1.0))
        }
        StringLastIndexOf => {
            let s = receiver.as_str().unwrap_or("");
            let search = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
            let from_idx = args.get(1).map(|v| if v.is_number() { (v.as_number() as usize).min(s.len()) } else { s.len() }).unwrap_or(s.len());
            let slice = if from_idx < s.len() { &s[..=from_idx] } else { s };
            if let Some(pos) = slice.rfind(search) {
                Ok(Value::number(pos as f64))
            } else {
                Ok(Value::number(-1.0))
            }
        }
        StringIncludes => {
            let s = receiver.as_str().unwrap_or("");
            let search = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
            let from_idx = args.get(1).map(|v| if v.is_number() { (v.as_number() as usize).min(s.len()) } else { 0 }).unwrap_or(0);
            if from_idx <= s.len() {
                Ok(Value::boolean(s[from_idx..].contains(search)))
            } else {
                Ok(Value::boolean(false))
            }
        }
        StringStartsWith => {
            let s = receiver.as_str().unwrap_or("");
            let prefix = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
            let from_idx = args.get(1).map(|v| if v.is_number() { (v.as_number() as usize).min(s.len()) } else { 0 }).unwrap_or(0);
            if from_idx <= s.len() {
                Ok(Value::boolean(s[from_idx..].starts_with(prefix)))
            } else {
                Ok(Value::boolean(false))
            }
        }
        StringEndsWith => {
            let s = receiver.as_str().unwrap_or("");
            let suffix = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
            let end_idx = args.get(1).map(|v| if v.is_number() { (v.as_number() as usize).min(s.len()) } else { s.len() }).unwrap_or(s.len());
            let slice = if end_idx <= s.len() { &s[..end_idx] } else { s };
            Ok(Value::boolean(slice.ends_with(suffix)))
        }
        StringReplace => {
            let s = receiver.as_str().unwrap_or("");
            let search = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
            let replace = args.get(1).and_then(|v| v.as_str()).unwrap_or("");
            let res = s.replacen(search, replace, 1);
            let ptr = gc_alloc_string(&res);
            Ok(Value::string(ptr))
        }
        StringReplaceAll => {
            let s = receiver.as_str().unwrap_or("");
            let search = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
            let replace = args.get(1).and_then(|v| v.as_str()).unwrap_or("");
            let res = s.replace(search, replace);
            let ptr = gc_alloc_string(&res);
            Ok(Value::string(ptr))
        }
        StringCharAt => {
            let s = receiver.as_str().unwrap_or("");
            let idx = args.get(0).map(|v| if v.is_number() { v.as_number() as usize } else { 0 }).unwrap_or(0);
            if let Some(c) = s.chars().nth(idx) {
                let mut buf = String::new();
                buf.push(c);
                let ptr = gc_alloc_string(&buf);
                Ok(Value::string(ptr))
            } else {
                let ptr = gc_alloc_string("");
                Ok(Value::string(ptr))
            }
        }
        StringCharCodeAt => {
            let s = receiver.as_str().unwrap_or("");
            let idx = args.get(0).map(|v| if v.is_number() { v.as_number() as usize } else { 0 }).unwrap_or(0);
            if let Some(c) = s.chars().nth(idx) {
                Ok(Value::number(c as u32 as f64))
            } else {
                Ok(Value::null())
            }
        }
        StringRepeat => {
            let s = receiver.as_str().unwrap_or("");
            let count = args.get(0).map(|v| if v.is_number() { (v.as_number() as usize).max(0) } else { 0 }).unwrap_or(0);
            let res = s.repeat(count);
            let ptr = gc_alloc_string(&res);
            Ok(Value::string(ptr))
        }
        StringPadStart => {
            let s = receiver.as_str().unwrap_or("");
            let target_len = args.get(0).map(|v| if v.is_number() { (v.as_number() as usize).max(0) } else { 0 }).unwrap_or(0);
            let pad_str = args.get(1).and_then(|v| v.as_str()).unwrap_or(" ");
            let curr_len = s.chars().count();
            if curr_len >= target_len || pad_str.is_empty() {
                return Ok(receiver);
            }
            let needed = target_len - curr_len;
            let mut pad = String::new();
            while pad.chars().count() < needed {
                pad.push_str(pad_str);
            }
            let pad_trimmed: String = pad.chars().take(needed).collect();
            let res = format!("{}{}", pad_trimmed, s);
            let ptr = gc_alloc_string(&res);
            Ok(Value::string(ptr))
        }
        StringPadEnd => {
            let s = receiver.as_str().unwrap_or("");
            let target_len = args.get(0).map(|v| if v.is_number() { (v.as_number() as usize).max(0) } else { 0 }).unwrap_or(0);
            let pad_str = args.get(1).and_then(|v| v.as_str()).unwrap_or(" ");
            let curr_len = s.chars().count();
            if curr_len >= target_len || pad_str.is_empty() {
                return Ok(receiver);
            }
            let needed = target_len - curr_len;
            let mut pad = String::new();
            while pad.chars().count() < needed {
                pad.push_str(pad_str);
            }
            let pad_trimmed: String = pad.chars().take(needed).collect();
            let res = format!("{}{}", s, pad_trimmed);
            let ptr = gc_alloc_string(&res);
            Ok(Value::string(ptr))
        }
        StringConcat => {
            let mut res = receiver.as_str().unwrap_or("").to_string();
            for arg in args {
                if let Some(s) = arg.as_str() {
                    res.push_str(s);
                } else {
                    res.push_str(&arg.to_string());
                }
            }
            let ptr = gc_alloc_string(&res);
            Ok(Value::string(ptr))
        }
        _ => Err("Invalid string builtin method".to_string()),
    }
}
