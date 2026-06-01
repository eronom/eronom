use std::time::Instant;
use crate::vm::execute::VM;
use crate::vm::value::{Value, MapKey, push_positive_integer, ADD_SCRATCH};
use crate::vm::gc::{gc_allocate, gc_write_barrier, GcData, get_or_create_string, get_pooled_vec, get_pooled_map, GC_NEEDS_STEP};
use super::profile::{JIT_PROFILING, JIT_PROFILER};

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_negate(vm: *mut VM, val: Value) -> Value {
    unsafe {
        if val.is_number() {
            Value::number_unchecked(-val.as_number())
        } else {
            (*vm).error = Some("Operand must be a number".into());
            Value::null()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_not(_vm: *mut VM, val: Value) -> Value {
    let res = if val.is_boolean() {
        !val.as_boolean()
    } else if val.is_null() {
        true
    } else {
        false
    };
    Value::boolean(res)
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_add(vm: *mut VM, val_b: Value, val_c: Value) -> Value {
    let start_time = if JIT_PROFILING { Some(Instant::now()) } else { None };
    unsafe {
        let res = if val_b.is_number() && val_c.is_number() {
            Value::number_unchecked(val_b.as_number() + val_c.as_number())
        } else {
            use std::fmt::Write;
            if val_b.is_string() {
                let sa_str = match &(*val_b.as_gc_ptr()).data {
                    GcData::String(s) => s,
                    _ => unreachable!(),
                };
                let new_ptr = ADD_SCRATCH.with(|scratch| {
                    let mut s_ref = scratch.borrow_mut();
                    s_ref.clear();
                    s_ref.push_str(sa_str);
                    if val_c.is_string() {
                        let sb_str = match &(*val_c.as_gc_ptr()).data {
                            GcData::String(s) => s,
                            _ => unreachable!(),
                        };
                        s_ref.push_str(sb_str);
                    } else if val_c.is_number() {
                        let val = val_c.as_number();
                        if val >= 0.0 && val == val.trunc() && val < 1.8446744073709552e19 {
                            push_positive_integer(&mut s_ref, val as u64);
                        } else {
                            let _ = write!(&mut s_ref, "{}", val);
                        }
                    } else {
                        let _ = write!(&mut s_ref, "{}", val_c);
                    }
                    get_or_create_string(s_ref.as_str())
                });
                Value::string(new_ptr)
            } else if val_c.is_string() {
                let sb_str = match &(*val_c.as_gc_ptr()).data {
                    GcData::String(s) => s,
                    _ => unreachable!(),
                };
                let new_ptr = ADD_SCRATCH.with(|scratch| {
                    let mut s_ref = scratch.borrow_mut();
                    s_ref.clear();
                    if val_b.is_number() {
                        let val = val_b.as_number();
                        if val >= 0.0 && val == val.trunc() && val < 1.8446744073709552e19 {
                            push_positive_integer(&mut s_ref, val as u64);
                        } else {
                            let _ = write!(&mut s_ref, "{}", val);
                        }
                    } else {
                        let _ = write!(&mut s_ref, "{}", val_b);
                    }
                    s_ref.push_str(sb_str);
                    get_or_create_string(s_ref.as_str())
                });
                Value::string(new_ptr)
            } else {
                (*vm).error = Some("Operands must be numbers or strings".into());
                Value::null()
            }
        };
        if JIT_PROFILING {
            JIT_PROFILER.with(|p| {
                let mut s = p.borrow_mut();
                s.add_count += 1;
                s.add_time += start_time.unwrap().elapsed();
            });
        }
        res
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_sub(vm: *mut VM, val_b: Value, val_c: Value) -> Value {
    unsafe {
        if val_b.is_number() && val_c.is_number() {
            Value::number_unchecked(val_b.as_number() - val_c.as_number())
        } else {
            (*vm).error = Some("Operands must be numbers".into());
            Value::null()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_mul(vm: *mut VM, val_b: Value, val_c: Value) -> Value {
    unsafe {
        if val_b.is_number() && val_c.is_number() {
            Value::number_unchecked(val_b.as_number() * val_c.as_number())
        } else {
            (*vm).error = Some("Operands must be numbers".into());
            Value::null()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_div(vm: *mut VM, val_b: Value, val_c: Value) -> Value {
    unsafe {
        if val_b.is_number() && val_c.is_number() {
            Value::number_unchecked(val_b.as_number() / val_c.as_number())
        } else {
            (*vm).error = Some("Operands must be numbers".into());
            Value::null()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_equal(_vm: *mut VM, val_b: Value, val_c: Value) -> Value {
    Value::boolean(val_b == val_c)
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_greater(vm: *mut VM, val_b: Value, val_c: Value) -> Value {
    unsafe {
        if val_b.is_number() && val_c.is_number() {
            Value::boolean(val_b.as_number() > val_c.as_number())
        } else {
            (*vm).error = Some("Operands must be numbers".into());
            Value::null()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_less(vm: *mut VM, val_b: Value, val_c: Value) -> Value {
    unsafe {
        if val_b.is_number() && val_c.is_number() {
            Value::boolean(val_b.as_number() < val_c.as_number())
        } else {
            (*vm).error = Some("Operands must be numbers".into());
            Value::null()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_define_global(vm: *mut VM, name_val: Value, val: Value) -> i64 {
    unsafe {
        let name = match &(*name_val.as_gc_ptr()).data {
            GcData::String(s) => s.clone(),
            _ => unreachable!(),
        };
        (*vm).globals.insert(name, val);
        0
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_get_global(vm: *mut VM, name_val: Value) -> Value {
    unsafe {
        let name = match &(*name_val.as_gc_ptr()).data {
            GcData::String(s) => s,
            _ => unreachable!(),
        };
        if let Some(val) = (*vm).globals.get(name) {
            *val
        } else {
            (*vm).error = Some(format!("Undefined variable '{}'", name));
            Value::null()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_set_global(vm: *mut VM, val: Value, name_val: Value) -> i64 {
    unsafe {
        let name = match &(*name_val.as_gc_ptr()).data {
            GcData::String(s) => s.clone(),
            _ => unreachable!(),
        };
        match (*vm).globals.entry(name.clone()) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                entry.insert(val);
                0
            }
            std::collections::hash_map::Entry::Vacant(_) => {
                (*vm).error = Some(format!(
                    "Variable '{}' not declared. It needs to be declared with 'let' or 'const'.",
                    name
                ));
                -1
            }
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_has_error(vm: *mut VM) -> i64 {
    unsafe {
        if (*vm).error.is_some() {
            1
        } else {
            0
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_needs_gc() -> i64 {
    unsafe { if GC_NEEDS_STEP { 1 } else { 0 } }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_make_array(_vm: *mut VM, start_reg: *const Value, count: i64) -> Value {
    let start_time = if JIT_PROFILING { Some(Instant::now()) } else { None };
    unsafe {
        let mut elements = get_pooled_vec(count as usize);
        let slice = std::slice::from_raw_parts(start_reg, count as usize);
        elements.extend_from_slice(slice);
        let ptr = gc_allocate(GcData::Array(elements));
        let res = Value::array(ptr);
        if JIT_PROFILING {
            JIT_PROFILER.with(|p| {
                let mut s = p.borrow_mut();
                s.make_array_count += 1;
                s.make_array_time += start_time.unwrap().elapsed();
            });
        }
        res
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_make_object(vm: *mut VM, start_reg: *const Value, count: i64) -> Value {
    let start_time = if JIT_PROFILING { Some(Instant::now()) } else { None };
    unsafe {
        let mut obj = get_pooled_map(count as usize);
        for i in 0..count {
            let key_val = *start_reg.offset((i * 2) as isize);
            let val = *start_reg.offset((i * 2 + 1) as isize);
            if !key_val.is_string() {
                (*vm).error = Some("Object key must be string".into());
                return Value::null();
            }
            obj.insert(MapKey(key_val), val);
        }
        let ptr = gc_allocate(GcData::Object(obj));
        let res = Value::object(ptr);
        if JIT_PROFILING {
            JIT_PROFILER.with(|p| {
                let mut s = p.borrow_mut();
                s.make_object_count += 1;
                s.make_object_time += start_time.unwrap().elapsed();
            });
        }
        res
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_get_property(vm: *mut VM, obj: Value, name_val: Value) -> Value {
    let start_time = if JIT_PROFILING { Some(Instant::now()) } else { None };
    unsafe {
        let res = if obj.is_object() {
            let ptr = obj.as_gc_ptr();
            let name = match &(*name_val.as_gc_ptr()).data {
                GcData::String(s) => s.as_ref(),
                _ => "",
            };
            let mut is_json_method = false;
            let mut is_text_method = false;
            if name == "json" || name == "text" {
                let body_key = get_or_create_string("_body");
                let is_response = match &(*ptr).data {
                    GcData::Object(map) => map.contains_key(&MapKey(Value::string(body_key))),
                    _ => false,
                };
                if is_response {
                    if name == "json" {
                        is_json_method = true;
                    } else {
                        is_text_method = true;
                    }
                }
            }
            if is_json_method {
                Value(crate::vm::value::TAG_METHOD_JSON | (ptr as u64 & crate::vm::value::PTR_MASK))
            } else if is_text_method {
                Value(crate::vm::value::TAG_METHOD_TEXT | (ptr as u64 & crate::vm::value::PTR_MASK))
            } else {
                match &(*ptr).data {
                    GcData::Object(map) => {
                        map.get(&MapKey(name_val)).cloned().unwrap_or(Value::null())
                    }
                    _ => unreachable!(),
                }
            }
        } else if obj.is_array() {
            let name = match &(*name_val.as_gc_ptr()).data {
                GcData::String(s) => s.as_ref(),
                _ => unreachable!(),
            };
            let ptr = obj.as_gc_ptr();
            match &(*ptr).data {
                GcData::Array(arr) => {
                    if name == "push" {
                        Value::array_method_push(ptr)
                    } else if name == "pop" {
                        Value::array_method_pop(ptr)
                    } else if name == "length" {
                        Value::number(arr.len() as f64)
                    } else if let Ok(idx) = name.parse::<usize>() {
                        arr.get(idx).cloned().unwrap_or(Value::null())
                    } else {
                        Value::null()
                    }
                }
                _ => unreachable!(),
            }
        } else {
            (*vm).error = Some("Only objects and arrays have properties".into());
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
                GcData::Object(map) => {
                    map.insert(MapKey(name_val), val);
                    gc_write_barrier(ptr, &val);
                    0
                }
                _ => unreachable!(),
            }
        } else if obj.is_array() {
            let name = match &(*name_val.as_gc_ptr()).data {
                GcData::String(s) => s.as_ref(),
                _ => unreachable!(),
            };
            let ptr = obj.as_gc_ptr();
            match &mut (*ptr).data {
                GcData::Array(arr) => {
                    if let Ok(idx) = name.parse::<usize>() {
                        if idx < arr.len() {
                            arr[idx] = val;
                        } else if idx == arr.len() {
                            arr.push(val);
                        } else {
                            (*vm).error = Some(format!(
                                "Index {} out of bounds for array of length {}",
                                idx,
                                arr.len()
                            ));
                            return -1;
                        }
                        gc_write_barrier(ptr, &val);
                        0
                    } else {
                        (*vm).error = Some("Cannot set non-numeric property on array".into());
                        -1
                    }
                }
                _ => unreachable!(),
            }
        } else {
            (*vm).error = Some("Only objects and arrays have properties".into());
            -1
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_get_index(vm: *mut VM, obj: Value, index: Value) -> Value {
    unsafe {
        if obj.is_array() {
            let ptr = obj.as_gc_ptr();
            if index.is_number() {
                let idx = index.as_number() as usize;
                match &(*ptr).data {
                    GcData::Array(arr) => {
                        arr.get(idx).cloned().unwrap_or(Value::null())
                    }
                    _ => unreachable!(),
                }
            } else if index.is_string() {
                let s = match &(*index.as_gc_ptr()).data {
                    GcData::String(st) => st,
                    _ => unreachable!(),
                };
                if let Ok(idx) = s.parse::<usize>() {
                    match &(*ptr).data {
                        GcData::Array(arr) => {
                            arr.get(idx).cloned().unwrap_or(Value::null())
                        }
                        _ => unreachable!(),
                    }
                } else {
                    Value::null()
                }
            } else {
                (*vm).error = Some("Only arrays can be indexed by numbers, and objects by strings".into());
                Value::null()
            }
        } else if obj.is_object() {
            let ptr = obj.as_gc_ptr();
            if index.is_string() {
                match &(*ptr).data {
                    GcData::Object(map) => {
                        map.get(&MapKey(index)).cloned().unwrap_or(Value::null())
                    }
                    _ => unreachable!(),
                }
            } else {
                (*vm).error = Some("Only arrays can be indexed by numbers, and objects by strings".into());
                Value::null()
            }
        } else {
            (*vm).error = Some("Only arrays can be indexed by numbers, and objects by strings".into());
            Value::null()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_set_index(vm: *mut VM, obj: Value, index: Value, val: Value) -> i64 {
    unsafe {
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
                            (*vm).error = Some(format!(
                                "Index {} out of bounds for array of length {}",
                                idx,
                                arr.len()
                            ));
                            return -1;
                        }
                        gc_write_barrier(ptr, &val);
                        0
                    }
                    _ => unreachable!(),
                }
            } else if index.is_string() {
                let s = match &(*index.as_gc_ptr()).data {
                    GcData::String(st) => st,
                    _ => unreachable!(),
                };
                if let Ok(idx) = s.parse::<usize>() {
                    match &mut (*ptr).data {
                        GcData::Array(arr) => {
                            if idx < arr.len() {
                                arr[idx] = val;
                            } else if idx == arr.len() {
                                arr.push(val);
                            } else {
                                (*vm).error = Some(format!(
                                    "Index {} out of bounds for array of length {}",
                                    idx,
                                    arr.len()
                                ));
                                return -1;
                            }
                            gc_write_barrier(ptr, &val);
                            0
                        }
                        _ => unreachable!(),
                    }
                } else {
                    (*vm).error = Some("Cannot set non-numeric property on array".into());
                    -1
                }
            } else {
                (*vm).error = Some("Only arrays can be indexed by numbers, and objects by strings".into());
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
                    _ => unreachable!(),
                }
            } else {
                (*vm).error = Some("Only arrays can be indexed by numbers, and objects by strings".into());
                -1
            }
        } else {
            (*vm).error = Some("Only arrays can be indexed by numbers, and objects by strings".into());
            -1
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_call_non_vm(
    _vm: *mut VM,
    dest: *mut Value,
    callee: Value,
    func_reg: i64,
    arg_count: i64,
    frame_slots: *mut Value,
) -> i64 {
    let start_time = if JIT_PROFILING { Some(Instant::now()) } else { None };
    unsafe {
        let status = if callee.is_native_function() {
            let native = callee.as_native_fn();
            let mut args = Vec::with_capacity(arg_count as usize);
            for i in 0..arg_count {
                args.push(*frame_slots.offset((func_reg + 1 + i) as isize));
            }
            let result = native(args);
            if (*_vm).stack.is_empty() {
                -3
            } else {
                *dest = result;
                0
            }
        } else if callee.is_method_json() || callee.is_method_text() {
            let ptr = callee.as_gc_ptr();
            let result = match &(*ptr).data {
                GcData::Object(map) => {
                    let body_key = get_or_create_string("_body");
                    let body_val = map.get(&MapKey(Value::string(body_key))).cloned().unwrap_or(Value::null());
                    if callee.is_method_json() {
                        if body_val.is_string() {
                            let s = match &(*body_val.as_gc_ptr()).data {
                                GcData::String(st) => st.as_ref(),
                                _ => "",
                            };
                            if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(s) {
                                crate::vm::gc::json_to_value(json_val)
                            } else {
                                Value::null()
                            }
                        } else {
                            Value::null()
                        }
                    } else {
                        body_val
                    }
                }
                _ => unreachable!(),
            };
            *dest = result;
            0
        } else if callee.is_method_send_json() {
            let res_ptr = (callee.0 & crate::vm::value::PTR_MASK) as *mut std::ffi::c_void;
            if !res_ptr.is_null() {
                let arg = if arg_count > 0 {
                    *frame_slots.offset((func_reg + 1) as isize)
                } else {
                    Value::null()
                };
                let json_val = crate::vm::er_http::value_to_json(arg);
                let json_str = serde_json::to_string(&json_val).unwrap_or_else(|_| "null".to_string());
                crate::vm::er_http::end_http_response_json(res_ptr, &json_str);
            }
            *dest = Value::null();
            0
        } else if callee.is_method_resolve() {
            let promise_ptr = callee.as_gc_ptr();
            let arg = if arg_count > 0 {
                *frame_slots.offset((func_reg + 1) as isize)
            } else {
                Value::null()
            };
            let queue = (*_vm).event_loop_queue.clone();
            let mut q = queue.lock().unwrap();
            q.push(crate::vm::execute::EventLoopTask {
                callback: Value::null(),
                args: Vec::new(),
                result: crate::vm::execute::AsyncResult::ResolvePromise(promise_ptr, arg),
            });
            *dest = Value::null();
            0
        } else if callee.is_array_method_push() || callee.is_array_method_pop() {
            let ptr = callee.as_gc_ptr();
            let result = match &mut (*ptr).data {
                GcData::Array(arr) => {
                    if callee.is_array_method_push() {
                        for i in 0..arg_count {
                            let arg = *frame_slots.offset((func_reg + 1 + i) as isize);
                            gc_write_barrier(ptr, &arg);
                            arr.push(arg);
                        }
                        Value::number(arr.len() as f64)
                    } else {
                        arr.pop().unwrap_or(Value::null())
                    }
                }
                _ => unreachable!(),
            };
            *dest = result;
            0
        } else {
            -1 // Not a native function or method, needs fallback
        };
        if JIT_PROFILING {
            JIT_PROFILER.with(|p| {
                let mut s = p.borrow_mut();
                s.call_non_vm_count += 1;
                s.call_non_vm_time += start_time.unwrap().elapsed();
            });
        }
        status
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_array_push(arr_val: Value, arg: Value) -> Value {
    let start_time = if JIT_PROFILING { Some(Instant::now()) } else { None };
    unsafe {
        let ptr = arr_val.as_gc_ptr();
        let res = match &mut (*ptr).data {
            GcData::Array(arr) => {
                gc_write_barrier(ptr, &arg);
                arr.push(arg);
                Value::number(arr.len() as f64)
            }
            _ => unreachable!(),
        };
        if JIT_PROFILING {
            JIT_PROFILER.with(|p| {
                let mut s = p.borrow_mut();
                s.call_non_vm_count += 1;
                s.call_non_vm_time += start_time.unwrap().elapsed();
            });
        }
        res
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_array_pop(arr_val: Value) -> Value {
    let start_time = if JIT_PROFILING { Some(Instant::now()) } else { None };
    unsafe {
        let ptr = arr_val.as_gc_ptr();
        let res = match &mut (*ptr).data {
            GcData::Array(arr) => {
                arr.pop().unwrap_or(Value::null())
            }
            _ => unreachable!(),
        };
        if JIT_PROFILING {
            JIT_PROFILER.with(|p| {
                let mut s = p.borrow_mut();
                s.call_non_vm_count += 1;
                s.call_non_vm_time += start_time.unwrap().elapsed();
            });
        }
        res
    }
}
