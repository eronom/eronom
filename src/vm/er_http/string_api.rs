use crate::vm::value::Value;
use crate::vm::gc::{gc_allocate, get_or_create_string, GcData};

pub fn native_string_split(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::null();
    }
    let s = match args[0].as_str() {
        Some(val) => val,
        None => return Value::null(),
    };
    let sep = match args[1].as_str() {
        Some(val) => val,
        None => return Value::null(),
    };
    let parts: Vec<Value> = s.split(sep)
        .map(|part| Value::string(get_or_create_string(part)))
        .collect();
    
    let ptr = gc_allocate(GcData::Array(parts));
    Value::array(ptr)
}

pub fn native_string_includes(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::boolean(false);
    }
    let s = match args[0].as_str() {
        Some(val) => val,
        None => return Value::boolean(false),
    };
    let search = match args[1].as_str() {
        Some(val) => val,
        None => return Value::boolean(false),
    };
    Value::boolean(s.contains(search))
}

pub fn native_string_starts_with(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::boolean(false);
    }
    let s = match args[0].as_str() {
        Some(val) => val,
        None => return Value::boolean(false),
    };
    let prefix = match args[1].as_str() {
        Some(val) => val,
        None => return Value::boolean(false),
    };
    Value::boolean(s.starts_with(prefix))
}

pub fn native_string_ends_with(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::boolean(false);
    }
    let s = match args[0].as_str() {
        Some(val) => val,
        None => return Value::boolean(false),
    };
    let suffix = match args[1].as_str() {
        Some(val) => val,
        None => return Value::boolean(false),
    };
    Value::boolean(s.ends_with(suffix))
}

pub fn native_string_substring(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::null();
    }
    let s = match args[0].as_str() {
        Some(val) => val,
        None => return Value::null(),
    };
    let start = args[1].as_number() as usize;
    let end = if args.len() >= 3 {
        args[2].as_number() as usize
    } else {
        s.len()
    };
    if start > s.len() || end > s.len() || start > end {
        return Value::null();
    }
    let sub = &s[start..end];
    let ptr = get_or_create_string(sub);
    Value::string(ptr)
}

pub fn native_string_replace(args: Vec<Value>) -> Value {
    if args.len() < 3 {
        return Value::null();
    }
    let s = match args[0].as_str() {
        Some(val) => val,
        None => return Value::null(),
    };
    let from = match args[1].as_str() {
        Some(val) => val,
        None => return Value::null(),
    };
    let to = match args[2].as_str() {
        Some(val) => val,
        None => return Value::null(),
    };
    let replaced = s.replace(from, to);
    let ptr = get_or_create_string(&replaced);
    Value::string(ptr)
}

pub fn native_string_trim(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let s = match args[0].as_str() {
        Some(val) => val,
        None => return Value::null(),
    };
    let trimmed = s.trim();
    let ptr = get_or_create_string(trimmed);
    Value::string(ptr)
}

pub fn native_string_length(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::number(0.0);
    }
    let s = match args[0].as_str() {
        Some(val) => val,
        None => return Value::number(0.0),
    };
    Value::number(s.len() as f64)
}

pub fn native_string_char_at(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::null();
    }
    let s = match args[0].as_str() {
        Some(val) => val,
        None => return Value::null(),
    };
    let idx = args[1].as_number() as usize;
    if let Some(c) = s.chars().nth(idx) {
        let mut buf = [0; 4];
        let c_str = c.encode_utf8(&mut buf);
        let ptr = get_or_create_string(c_str);
        Value::string(ptr)
    } else {
        Value::null()
    }
}

pub fn native_string_index_of(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::number(-1.0);
    }
    let s = match args[0].as_str() {
        Some(val) => val,
        None => return Value::number(-1.0),
    };
    let search = match args[1].as_str() {
        Some(val) => val,
        None => return Value::number(-1.0),
    };
    match s.find(search) {
        Some(idx) => Value::number(idx as f64),
        None => Value::number(-1.0),
    }
}
