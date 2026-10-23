use crate::vm::value::Value;
use crate::vm::gc::{gc_alloc_array, BuiltinMethodId, GcData};

pub fn get_object_builtin_method_id(name: &str) -> Option<BuiltinMethodId> {
    use BuiltinMethodId::*;
    match name {
        "keys" => Some(ObjectKeys),
        "values" => Some(ObjectValues),
        "entries" => Some(ObjectEntries),
        "hasOwnProperty" | "has" => Some(ObjectHasOwnProperty),
        _ => None,
    }
}

pub fn execute_object_method(
    receiver: Value,
    method: BuiltinMethodId,
    args: &[Value],
) -> Result<Value, String> {
    use BuiltinMethodId::*;
    match method {
        ObjectKeys => {
            let mut keys_vec = Vec::new();
            if receiver.is_object() {
                let ptr = receiver.as_gc_ptr();
                unsafe {
                    match &(*ptr).data {
                        GcData::Object(map) => {
                            for key in map.keys() {
                                keys_vec.push(key.0);
                            }
                        }
                        GcData::Struct(s) => {
                            for key in s.descriptor.field_indices.keys() {
                                keys_vec.push(key.0);
                            }
                        }
                        _ => {}
                    }
                }
            }
            let ptr = gc_alloc_array(&keys_vec);
            Ok(Value::array(ptr))
        }
        ObjectValues => {
            let mut vals_vec = Vec::new();
            if receiver.is_object() {
                let ptr = receiver.as_gc_ptr();
                unsafe {
                    match &(*ptr).data {
                        GcData::Object(map) => {
                            for val in map.values() {
                                vals_vec.push(*val);
                            }
                        }
                        GcData::Struct(s) => {
                            for val in &s.fields {
                                vals_vec.push(*val);
                            }
                        }
                        _ => {}
                    }
                }
            }
            let ptr = gc_alloc_array(&vals_vec);
            Ok(Value::array(ptr))
        }
        ObjectEntries => {
            let mut entries_vec = Vec::new();
            if receiver.is_object() {
                let ptr = receiver.as_gc_ptr();
                unsafe {
                    match &(*ptr).data {
                        GcData::Object(map) => {
                            for (key, val) in map {
                                let pair_ptr = gc_alloc_array(&[key.0, *val]);
                                entries_vec.push(Value::array(pair_ptr));
                            }
                        }
                        GcData::Struct(s) => {
                            for (key, &idx) in &s.descriptor.field_indices {
                                let val = s.fields.get(idx).copied().unwrap_or(Value::null());
                                let pair_ptr = gc_alloc_array(&[key.0, val]);
                                entries_vec.push(Value::array(pair_ptr));
                            }
                        }
                        _ => {}
                    }
                }
            }
            let ptr = gc_alloc_array(&entries_vec);
            Ok(Value::array(ptr))
        }
        ObjectHasOwnProperty => {
            let key = args.get(0).copied().unwrap_or(Value::null());
            let mut has_prop = false;
            if receiver.is_object() {
                let ptr = receiver.as_gc_ptr();
                unsafe {
                    match &(*ptr).data {
                        GcData::Object(map) => {
                            has_prop = map.contains_key(&crate::vm::value::MapKey(key));
                        }
                        GcData::Struct(s) => {
                            has_prop = s.descriptor.field_indices.contains_key(&crate::vm::value::MapKey(key));
                        }
                        _ => {}
                    }
                }
            }
            Ok(Value::boolean(has_prop))
        }
        _ => Err("Invalid object builtin method".to_string()),
    }
}
