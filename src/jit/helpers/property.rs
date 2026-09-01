use std::time::Instant;
use crate::vm::execute::VM;
use crate::vm::value::{Value, MapKey};
use crate::vm::gc::{gc_allocate, gc_write_barrier, GcData, get_or_create_string, get_pooled_vec};
use crate::jit::profile::{JIT_PROFILING, JIT_PROFILER};

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_get_property(vm: *mut VM, obj: Value, name_val: Value) -> Value {
    if (obj.0 & 0xffff_0000_0000_0000) == crate::vm::value::TAG_OBJECT {
        let ptr = (obj.0 & crate::vm::value::PTR_MASK) as *mut crate::vm::gc::GcObject;
        unsafe {
            if let GcData::Struct(s) = &(*ptr).data {
                let count = s.descriptor.fast_field_count;
                let fast = &s.descriptor.fast_fields;
                for i in 0..count {
                    if fast[i].0.0 == name_val.0 {
                        return s.fields[fast[i].1];
                    }
                }
            }
        }
    }
    er_jit_get_property_slow(vm, obj, name_val)
}

pub fn er_jit_get_property_slow(vm: *mut VM, obj: Value, name_val: Value) -> Value {
    let start_time = if JIT_PROFILING { Some(Instant::now()) } else { None };
    unsafe {
        let res = if obj.is_object() {
            let ptr = obj.as_gc_ptr();
            match &(*ptr).data {
                GcData::Struct(s) => {
                    if let Some(val) = s.get_field(name_val) {
                        val
                    } else if let Some(&method_val) = s.descriptor.methods.get(&MapKey(name_val)) {
                        let bound_method = crate::vm::gc::GcBoundMethod {
                            receiver: obj,
                            function: method_val.as_gc_ptr(),
                        };
                        let ptr = gc_allocate(GcData::BoundMethod(bound_method));
                        Value::function(ptr)
                    } else {
                        let name = name_val.as_str().unwrap_or("");
                        if name == "json" || name == "text" {
                            if s.get_field_by_name("_body").is_some() {
                                let tag = if name == "json" { crate::vm::value::TAG_METHOD_JSON } else { crate::vm::value::TAG_METHOD_TEXT };
                                Value(tag | (ptr as u64 & crate::vm::value::PTR_MASK))
                            } else {
                                Value::null()
                            }
                        } else if name == "exists" && s.descriptor.name.as_ref() == "File" {
                            Value(crate::vm::value::TAG_METHOD_FILE | (ptr as u64 & crate::vm::value::PTR_MASK & !3) | 0)
                        } else if let Some(m) = crate::vm::execute::get_object_builtin_method_id(name) {
                            let ptr = crate::vm::gc::gc_alloc_builtin_method(obj, m);
                            Value::function(ptr)
                        } else {
                            Value::null()
                        }
                    }
                }
                GcData::Object(map) => {
                    if let Some(&val) = map.get(&MapKey(name_val)) {
                        val
                    } else {
                        // Check response / file methods only on miss
                        let name = name_val.as_str().unwrap_or("");
                        if name == "json" || name == "text" {
                            let body_key = get_or_create_string("_body");
                            if map.contains_key(&MapKey(Value::string(body_key))) {
                                let tag = if name == "json" { crate::vm::value::TAG_METHOD_JSON } else { crate::vm::value::TAG_METHOD_TEXT };
                                Value(tag | (ptr as u64 & crate::vm::value::PTR_MASK))
                            } else {
                                Value::null()
                            }
                        } else if name == "exists" {
                            let file_key = get_or_create_string("_isFile");
                            if map.get(&MapKey(Value::string(file_key))).map(|v| v.as_boolean()).unwrap_or(false) {
                                Value(crate::vm::value::TAG_METHOD_FILE | (ptr as u64 & crate::vm::value::PTR_MASK & !3) | 0)
                            } else {
                                Value::null()
                            }
                        } else if let Some(m) = crate::vm::execute::get_object_builtin_method_id(name) {
                            let ptr = crate::vm::gc::gc_alloc_builtin_method(obj, m);
                            Value::function(ptr)
                        } else {
                            Value::null()
                        }
                    }
                }
                _ => Value::null(),
            }
        } else if obj.is_array() {
            let name = name_val.as_str().unwrap_or("");
            let ptr = obj.as_gc_ptr();
            match &(*ptr).data {
                GcData::Array(arr) => {
                    if name == "push" {
                        Value::array_method_push(ptr)
                    } else if name == "pop" {
                        Value::array_method_pop(ptr)
                    } else if name == "length" {
                        Value::number(arr.len() as f64)
                    } else if let Some(m) = crate::vm::execute::get_array_builtin_method_id(name) {
                        let ptr = crate::vm::gc::gc_alloc_builtin_method(obj, m);
                        Value::function(ptr)
                    } else if let Ok(idx) = name.parse::<usize>() {
                        arr.get(idx).cloned().unwrap_or(Value::null())
                    } else {
                        Value::null()
                    }
                }
                _ => Value::null(),
            }
        } else if obj.is_string() {
            let name = name_val.as_str().unwrap_or("");
            if name == "length" {
                let s = obj.as_str().unwrap_or("");
                Value::number(s.chars().count() as f64)
            } else if let Some(m) = crate::vm::execute::get_string_builtin_method_id(name) {
                let ptr = crate::vm::gc::gc_alloc_builtin_method(obj, m);
                Value::function(ptr)
            } else {
                Value::null()
            }
        } else {
            (*vm).has_error_flag = 1; (*vm).error = Some("Only objects, arrays, and strings have properties".into());
            Value::null()
        };
        if JIT_PROFILING {
            JIT_PROFILER.with(|p| {
                let mut s = p.borrow_mut();
                s.get_property_count += 1;
                s.get_property_time += start_time.unwrap().elapsed();
            });
        }
        res
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_set_property(vm: *mut VM, obj: Value, val: Value, name_val: Value) -> i64 {
    unsafe {
        if obj.is_object() {
            let ptr = obj.as_gc_ptr();
            match &mut (*ptr).data {
                GcData::Struct(s) => {
                    if s.set_field(name_val, val) {
                        gc_write_barrier(ptr, &val);
                        0
                    } else if s.descriptor.name.as_ref() == "Anonymous" {
                        let new_desc = crate::vm::shape::transition_shape_add_property(&s.descriptor, name_val);
                        s.descriptor = new_desc;
                        s.fields.push(val);
                        gc_write_barrier(ptr, &val);
                        0
                    } else {
                        let name = name_val.as_str().unwrap_or("");
                        (*vm).has_error_flag = 1; (*vm).error = Some(format!("Struct has no field '{}'", name));
                        -1
                    }
                }
                GcData::Object(map) => {
                    map.insert(MapKey(name_val), val);
                    gc_write_barrier(ptr, &val);
                    0
                }
                _ => 0,
            }
        } else if obj.is_array() {
            let name = name_val.as_str().unwrap_or("");
            let ptr = obj.as_gc_ptr();
            match &mut (*ptr).data {
                GcData::Array(arr) => {
                    if let Ok(idx) = name.parse::<usize>() {
                        if idx < arr.len() {
                            arr[idx] = val;
                        } else if idx == arr.len() {
                            arr.push(val);
                        } else {
                            (*vm).has_error_flag = 1; (*vm).error = Some(format!(
                                "Index {} out of bounds for array of length {}",
                                idx,
                                arr.len()
                            ));
                            return -1;
                        }
                        gc_write_barrier(ptr, &val);
                        0
                    } else {
                        (*vm).has_error_flag = 1; (*vm).error = Some("Cannot set non-numeric property on array".into());
                        -1
                    }
                }
                _ => -1,
            }
        } else {
            (*vm).has_error_flag = 1; (*vm).error = Some("Only objects and arrays have properties".into());
            -1
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_get_index(vm: *mut VM, obj: Value, index: Value) -> Value {
    unsafe {
        if (obj.0 & 0xffff_0000_0000_0000) == crate::vm::value::TAG_ARRAY && index.0 < crate::vm::value::TAG_NUMBER_MASK {
            let ptr = (obj.0 & crate::vm::value::PTR_MASK) as *mut crate::vm::gc::GcObject;
            if let GcData::Array(arr) = &(*ptr).data {
                let idx = index.as_number() as usize;
                if idx < arr.len() {
                    return *arr.as_ptr().add(idx);
                }
            }
            return Value::null();
        }

        if obj.is_array() {
            let ptr = obj.as_gc_ptr();
            if index.is_number() {
                let idx = index.as_number() as usize;
                match &(*ptr).data {
                    GcData::Array(arr) => {
                        arr.get(idx).cloned().unwrap_or(Value::null())
                    }
                    _ => Value::null(),
                }
            } else if index.is_string() {
                let s = index.as_str().unwrap_or("");
                if let Ok(idx) = s.parse::<usize>() {
                    match &(*ptr).data {
                        GcData::Array(arr) => {
                            arr.get(idx).cloned().unwrap_or(Value::null())
                        }
                        _ => Value::null(),
                    }
                } else {
                    Value::null()
                }
            } else {
                (*vm).has_error_flag = 1; (*vm).error = Some("Only arrays can be indexed by numbers, and objects by strings".into());
                Value::null()
            }
        } else if obj.is_object() {
            let ptr = obj.as_gc_ptr();
            if index.is_string() {
                match &(*ptr).data {
                    GcData::Object(map) => {
                        map.get(&MapKey(index)).cloned().unwrap_or(Value::null())
                    }
                    GcData::Struct(s) => {
                        s.get_field(index).unwrap_or(Value::null())
                    }
                    _ => Value::null(),
                }
            } else {
                (*vm).has_error_flag = 1; (*vm).error = Some("Only arrays can be indexed by numbers, and objects by strings".into());
                Value::null()
            }
        } else {
            (*vm).has_error_flag = 1; (*vm).error = Some("Only arrays can be indexed by numbers, and objects by strings".into());
            Value::null()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_set_index(vm: *mut VM, obj: Value, index: Value, val: Value) -> i64 {
    unsafe {
        if (obj.0 & 0xffff_0000_0000_0000) == crate::vm::value::TAG_ARRAY && index.0 < crate::vm::value::TAG_NUMBER_MASK {
            let ptr = (obj.0 & crate::vm::value::PTR_MASK) as *mut crate::vm::gc::GcObject;
            if let GcData::Array(arr) = &mut (*ptr).data {
                let idx = index.as_number() as usize;
                if idx < arr.len() {
                    *arr.as_mut_ptr().add(idx) = val;
                    gc_write_barrier(ptr, &val);
                    return 0;
                } else if idx == arr.len() {
                    arr.push(val);
                    gc_write_barrier(ptr, &val);
                    return 0;
                }
            }
        }

        if obj.is_array() {
            let ptr = obj.as_gc_ptr();
            if index.is_number() {
                let idx = index.as_number() as usize;
                match &mut (*ptr).data {
                    GcData::Array(arr) => {
                        if idx < arr.len() {
                            arr[idx] = val;
                        } else if idx == arr.len() {
                            arr.push(val);
                        } else {
                            (*vm).has_error_flag = 1; (*vm).error = Some(format!(
                                "Index {} out of bounds for array of length {}",
                                idx,
                                arr.len()
                            ));
                            return -1;
                        }
                        gc_write_barrier(ptr, &val);
                        0
                    }
                    _ => -1,
                }
            } else if index.is_string() {
                let s = index.as_str().unwrap_or("");
                if let Ok(idx) = s.parse::<usize>() {
                    match &mut (*ptr).data {
                        GcData::Array(arr) => {
                            if idx < arr.len() {
                                arr[idx] = val;
                            } else if idx == arr.len() {
                                arr.push(val);
                            } else {
                                (*vm).has_error_flag = 1; (*vm).error = Some(format!(
                                    "Index {} out of bounds for array of length {}",
                                    idx,
                                    arr.len()
                                ));
                                return -1;
                            }
                            gc_write_barrier(ptr, &val);
                            0
                        }
                        _ => -1,
                    }
                } else {
                    (*vm).has_error_flag = 1; (*vm).error = Some("Cannot set non-numeric property on array".into());
                    -1
                }
            } else {
                (*vm).has_error_flag = 1; (*vm).error = Some("Only arrays can be indexed by numbers, and objects by strings".into());
                -1
            }
        } else if obj.is_object() {
            let ptr = obj.as_gc_ptr();
            if index.is_string() {
                match &mut (*ptr).data {
                    GcData::Object(map) => {
                        map.insert(MapKey(index), val);
                        gc_write_barrier(ptr, &val);
                        0
                    }
                    GcData::Struct(s) => {
                        if s.set_field(index, val) {
                            gc_write_barrier(ptr, &val);
                            0
                        } else if s.descriptor.name.as_ref() == "Anonymous" {
                            let new_desc = crate::vm::shape::transition_shape_add_property(&s.descriptor, index);
                            s.descriptor = new_desc;
                            s.fields.push(val);
                            gc_write_barrier(ptr, &val);
                            0
                        } else {
                            let name = index.as_str().unwrap_or("");
                            (*vm).has_error_flag = 1; (*vm).error = Some(format!("Struct has no field '{}'", name));
                            -1
                        }
                    }
                    _ => -1,
                }
            } else {
                (*vm).has_error_flag = 1; (*vm).error = Some("Only arrays can be indexed by numbers, and objects by strings".into());
                -1
            }
        } else {
            (*vm).has_error_flag = 1; (*vm).error = Some("Only arrays can be indexed by numbers, and objects by strings".into());
            -1
        }
    }
}

pub fn construct_struct_from_args_helper(
    descriptor: &std::rc::Rc<crate::vm::gc::StructDescriptor>,
    args: Vec<Value>,
) -> Result<Value, String> {
    if args.is_empty() {
        let count = descriptor.field_indices.len();
        let mut fields = get_pooled_vec(count);
        fields.resize(count, Value::null());
        let s_ptr = gc_allocate(GcData::Struct(crate::vm::gc::GcStruct {
            descriptor: descriptor.clone(),
            fields,
        }));
        return Ok(Value::object(s_ptr));
    }

    if args.len() == 1 {
        let arg = args[0];
        if arg.is_object() {
            let arg_ptr = arg.as_gc_ptr();
            unsafe {
                match &(*arg_ptr).data {
                    GcData::Object(map) => {
                        let count = descriptor.field_indices.len();
                        let mut fields = get_pooled_vec(count);
                        fields.resize(count, Value::null());
                        for (map_key, &idx) in &descriptor.field_indices {
                            if let Some(&val) = map.get(map_key) {
                                fields[idx] = val;
                            }
                        }
                        let s_ptr = gc_allocate(GcData::Struct(crate::vm::gc::GcStruct {
                            descriptor: descriptor.clone(),
                            fields,
                        }));
                        return Ok(Value::object(s_ptr));
                    }
                    GcData::Struct(s) => {
                        let count = descriptor.field_indices.len();
                        let mut fields = get_pooled_vec(count);
                        fields.resize(count, Value::null());
                        for (map_key, &idx) in &descriptor.field_indices {
                            if let Some(val) = s.get_field(map_key.0) {
                                fields[idx] = val;
                            }
                        }
                        let s_ptr = gc_allocate(GcData::Struct(crate::vm::gc::GcStruct {
                            descriptor: descriptor.clone(),
                            fields,
                        }));
                        return Ok(Value::object(s_ptr));
                    }
                    _ => {}
                }
            }
        } else if arg.is_array() {
            let arg_ptr = arg.as_gc_ptr();
            let mut mapped_elements = Vec::new();
            unsafe {
                if let GcData::Array(arr) = &(*arg_ptr).data {
                    for &item in arr {
                        let constructed = construct_struct_from_args_helper(descriptor, vec![item])?;
                        mapped_elements.push(constructed);
                    }
                }
            }
            let array_ptr = gc_allocate(GcData::Array(mapped_elements));
            return Ok(Value::array(array_ptr));
        }
    }

    let count = descriptor.field_indices.len();
    let mut fields = get_pooled_vec(count);
    fields.resize(count, Value::null());
    for i in 0..args.len().min(count) {
        fields[i] = args[i];
    }
    let s_ptr = gc_allocate(GcData::Struct(crate::vm::gc::GcStruct {
        descriptor: descriptor.clone(),
        fields,
    }));
    Ok(Value::object(s_ptr))
}
