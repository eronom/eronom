use serde::Serialize;
use super::gc::{gc_alloc_string, get_or_create_string, get_pooled_map, json_to_value, GcData};
use super::value::{MapKey, Value};
use super::execute::VM;

pub fn value_to_serde(val: Value) -> serde_json::Value {
    if val.is_null() {
        serde_json::Value::Null
    } else if val.is_boolean() {
        serde_json::Value::Bool(val.as_boolean())
    } else if val.is_number() {
        let n = val.as_number();
        if let Some(num) = serde_json::Number::from_f64(n) {
            serde_json::Value::Number(num)
        } else {
            serde_json::Value::Null
        }
    } else if let Some(s) = val.as_str() {
        serde_json::Value::String(s.to_string())
    } else if val.is_array() {
        unsafe {
            if let GcData::Array(ref arr) = (*val.as_gc_ptr()).data {
                let items: Vec<serde_json::Value> = arr.iter().map(|&v| value_to_serde(v)).collect();
                serde_json::Value::Array(items)
            } else {
                serde_json::Value::Array(Vec::new())
            }
        }
    } else if val.is_object() {
        unsafe {
            match (*val.as_gc_ptr()).data {
                GcData::Object(ref map) => {
                    let mut obj = serde_json::Map::new();
                    for (k, &v) in map {
                        if let Some(key_str) = k.0.as_str() {
                            obj.insert(key_str.to_string(), value_to_serde(v));
                        }
                    }
                    serde_json::Value::Object(obj)
                }
                GcData::Struct(ref s) => {
                    let mut obj = serde_json::Map::new();
                    for (map_key, &idx) in &s.descriptor.field_indices {
                        let name = map_key.0.as_str().unwrap_or("");
                        obj.insert(name.to_string(), value_to_serde(s.fields[idx]));
                    }
                    serde_json::Value::Object(obj)
                }
                _ => serde_json::Value::Object(serde_json::Map::new()),
            }
        }
    } else {
        serde_json::Value::Null
    }
}

pub fn native_json_parse(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let s = match args[0].as_str() {
        Some(val) => val,
        None => return Value::null(),
    };

    match serde_json::from_str::<serde_json::Value>(s) {
        Ok(json_val) => json_to_value(json_val),
        Err(_) => Value::null(),
    }
}

pub fn native_json_stringify(args: Vec<Value>) -> Value {
    if args.is_empty() {
        let ptr = gc_alloc_string("null");
        return Value::string(ptr);
    }

    let serde_val = value_to_serde(args[0]);

    let stringified = if args.len() > 1 && args[1].is_number() && args[1].as_number() > 0.0 {
        let indent_num = args[1].as_number() as usize;
        let indent = " ".repeat(indent_num);
        let mut buf = Vec::new();
        let formatter = serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes());
        let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
        if serde_val.serialize(&mut ser).is_ok() {
            String::from_utf8(buf).unwrap_or_else(|_| "null".to_string())
        } else {
            "null".to_string()
        }
    } else if args.len() > 1 && args[1].is_string() {
        let indent = args[1].as_str().unwrap_or("  ");
        let mut buf = Vec::new();
        let formatter = serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes());
        let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
        if serde_val.serialize(&mut ser).is_ok() {
            String::from_utf8(buf).unwrap_or_else(|_| "null".to_string())
        } else {
            "null".to_string()
        }
    } else {
        serde_json::to_string(&serde_val).unwrap_or_else(|_| "null".to_string())
    };

    let ptr = gc_alloc_string(&stringified);
    Value::string(ptr)
}

pub fn create_json_object() -> Value {
    let mut map = get_pooled_map(2);
    let parse_key = MapKey(Value::string(get_or_create_string("parse")));
    let parse_fn = Value::native_function(native_json_parse);
    map.insert(parse_key, parse_fn);

    let stringify_key = MapKey(Value::string(get_or_create_string("stringify")));
    let stringify_fn = Value::native_function(native_json_stringify);
    map.insert(stringify_key, stringify_fn);

    let ptr = super::gc::gc_allocate(GcData::Object(map));
    Value::object(ptr)
}

pub fn register_json_natives(vm: &mut VM) {
    vm.register_global("Eronom_nativeJsonParse", Value::native_function(native_json_parse));
    vm.register_global("Eronom_nativeJsonStringify", Value::native_function(native_json_stringify));
    vm.register_global("JSON", create_json_object());
}
