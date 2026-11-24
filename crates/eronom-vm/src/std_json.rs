// Standard JSON module builtins for Eronom VM runtime.

use super::gc::{gc_alloc_string, get_or_create_string, get_pooled_map};
use super::value::{MapKey, Value};

pub fn native_json_parse(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let s = match args[0].as_str() {
        Some(val) => val,
        None => return Value::null(),
    };

    crate::json_parse(s).unwrap_or(Value::null())
}

pub fn native_json_stringify(args: Vec<Value>) -> Value {
    if args.is_empty() {
        let ptr = gc_alloc_string("null");
        return Value::string(ptr);
    }

    let indent = if args.len() > 1 && args[1].is_number() && args[1].as_number() > 0.0 {
        Some(args[1].as_number() as usize)
    } else if args.len() > 1 && args[1].is_string() {
        Some(2)
    } else {
        None
    };

    let stringified = crate::json_stringify(args[0], indent);
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

    let obj = super::gc::gc_allocate(super::gc::GcData::Object(map));
    Value::object(obj)
}

pub fn register_json_natives(vm: &mut super::execute::VM) {
    vm.register_global("Eronom_nativeJsonParse", Value::native_function(native_json_parse));
    vm.register_global("Eronom_nativeJsonStringify", Value::native_function(native_json_stringify));
    vm.register_global("JSON", create_json_object());
}
