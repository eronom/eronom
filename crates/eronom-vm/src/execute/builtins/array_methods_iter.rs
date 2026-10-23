use crate::vm::value::Value;
use crate::vm::gc::{gc_alloc_array, gc_push_temp_slice, gc_pop_temp_slice, BuiltinMethodId, GcData};
use crate::vm::execute::types::VM;

pub fn execute_array_iter_method(
    vm: &mut VM,
    receiver: Value,
    method: BuiltinMethodId,
    args: &[Value],
) -> Result<Value, String> {
    use BuiltinMethodId::*;
    match method {
        ArrayMap => {
            let cb = args.get(0).copied().unwrap_or(Value::null());
            if !cb.is_function() && !cb.is_native_function() {
                return Err("Array.map requires a function callback".to_string());
            }
            let items: Vec<Value> = unsafe {
                match &(*receiver.as_gc_ptr()).data {
                    GcData::Array(arr) => arr.clone(),
                    _ => vec![],
                }
            };
            let mut mapped = Vec::with_capacity(items.len());
            for (i, item) in items.iter().enumerate() {
                gc_push_temp_slice(mapped.as_ptr(), mapped.len());
                let res = vm.call_function_reentrant(cb, vec![*item, Value::number(i as f64), receiver])?;
                gc_pop_temp_slice();
                mapped.push(res);
            }
            let ptr = gc_alloc_array(&mapped);
            Ok(Value::array(ptr))
        }
        ArrayFilter => {
            let cb = args.get(0).copied().unwrap_or(Value::null());
            if !cb.is_function() && !cb.is_native_function() {
                return Err("Array.filter requires a function callback".to_string());
            }
            let items: Vec<Value> = unsafe {
                match &(*receiver.as_gc_ptr()).data {
                    GcData::Array(arr) => arr.clone(),
                    _ => vec![],
                }
            };
            let mut filtered = Vec::new();
            for (i, item) in items.iter().enumerate() {
                gc_push_temp_slice(filtered.as_ptr(), filtered.len());
                let res = vm.call_function_reentrant(cb, vec![*item, Value::number(i as f64), receiver])?;
                gc_pop_temp_slice();
                if res.is_truthy() {
                    filtered.push(*item);
                }
            }
            let ptr = gc_alloc_array(&filtered);
            Ok(Value::array(ptr))
        }
        ArrayReduce => {
            let cb = args.get(0).copied().unwrap_or(Value::null());
            if !cb.is_function() && !cb.is_native_function() {
                return Err("Array.reduce requires a function callback".to_string());
            }
            let items: Vec<Value> = unsafe {
                match &(*receiver.as_gc_ptr()).data {
                    GcData::Array(arr) => arr.clone(),
                    _ => vec![],
                }
            };
            if items.is_empty() && args.len() < 2 {
                return Err("Reduce of empty array with no initial value".to_string());
            }
            let mut acc = if args.len() >= 2 {
                args[1]
            } else {
                items[0]
            };
            let start_idx = if args.len() >= 2 { 0 } else { 1 };
            for (i, item) in items.iter().enumerate().skip(start_idx) {
                gc_push_temp_slice(&acc as *const Value, 1);
                let res = vm.call_function_reentrant(cb, vec![acc, *item, Value::number(i as f64), receiver])?;
                gc_pop_temp_slice();
                acc = res;
            }
            Ok(acc)
        }
        ArrayForEach => {
            let cb = args.get(0).copied().unwrap_or(Value::null());
            if !cb.is_function() && !cb.is_native_function() {
                return Err("Array.forEach requires a function callback".to_string());
            }
            let items: Vec<Value> = unsafe {
                match &(*receiver.as_gc_ptr()).data {
                    GcData::Array(arr) => arr.clone(),
                    _ => vec![],
                }
            };
            for (i, item) in items.iter().enumerate() {
                vm.call_function_reentrant(cb, vec![*item, Value::number(i as f64), receiver])?;
            }
            Ok(Value::null())
        }
        ArrayFind => {
            let cb = args.get(0).copied().unwrap_or(Value::null());
            if !cb.is_function() && !cb.is_native_function() {
                return Err("Array.find requires a function callback".to_string());
            }
            let items: Vec<Value> = unsafe {
                match &(*receiver.as_gc_ptr()).data {
                    GcData::Array(arr) => arr.clone(),
                    _ => vec![],
                }
            };
            for (i, item) in items.iter().enumerate() {
                let res = vm.call_function_reentrant(cb, vec![*item, Value::number(i as f64), receiver])?;
                if res.is_truthy() {
                    return Ok(*item);
                }
            }
            Ok(Value::null())
        }
        ArrayFindIndex => {
            let cb = args.get(0).copied().unwrap_or(Value::null());
            if !cb.is_function() && !cb.is_native_function() {
                return Err("Array.findIndex requires a function callback".to_string());
            }
            let items: Vec<Value> = unsafe {
                match &(*receiver.as_gc_ptr()).data {
                    GcData::Array(arr) => arr.clone(),
                    _ => vec![],
                }
            };
            for (i, item) in items.iter().enumerate() {
                let res = vm.call_function_reentrant(cb, vec![*item, Value::number(i as f64), receiver])?;
                if res.is_truthy() {
                    return Ok(Value::number(i as f64));
                }
            }
            Ok(Value::number(-1.0))
        }
        ArraySome => {
            let cb = args.get(0).copied().unwrap_or(Value::null());
            if !cb.is_function() && !cb.is_native_function() {
                return Err("Array.some requires a function callback".to_string());
            }
            let items: Vec<Value> = unsafe {
                match &(*receiver.as_gc_ptr()).data {
                    GcData::Array(arr) => arr.clone(),
                    _ => vec![],
                }
            };
            for (i, item) in items.iter().enumerate() {
                let res = vm.call_function_reentrant(cb, vec![*item, Value::number(i as f64), receiver])?;
                if res.is_truthy() {
                    return Ok(Value::boolean(true));
                }
            }
            Ok(Value::boolean(false))
        }
        ArrayEvery => {
            let cb = args.get(0).copied().unwrap_or(Value::null());
            if !cb.is_function() && !cb.is_native_function() {
                return Err("Array.every requires a function callback".to_string());
            }
            let items: Vec<Value> = unsafe {
                match &(*receiver.as_gc_ptr()).data {
                    GcData::Array(arr) => arr.clone(),
                    _ => vec![],
                }
            };
            for (i, item) in items.iter().enumerate() {
                let res = vm.call_function_reentrant(cb, vec![*item, Value::number(i as f64), receiver])?;
                if !res.is_truthy() {
                    return Ok(Value::boolean(false));
                }
            }
            Ok(Value::boolean(true))
        }
        ArrayFlatMap => {
            let cb = args.get(0).copied().unwrap_or(Value::null());
            if !cb.is_function() && !cb.is_native_function() {
                return Err("Array.flatMap requires a function callback".to_string());
            }
            let items: Vec<Value> = unsafe {
                match &(*receiver.as_gc_ptr()).data {
                    GcData::Array(arr) => arr.clone(),
                    _ => vec![],
                }
            };
            let mut mapped = Vec::with_capacity(items.len());
            let mapped_ptr = &mapped as *const Vec<Value>;
            crate::vm::gc::GC_ROOTS.with(|roots| {
                roots.borrow_mut().push(Box::new(move || {
                    let vec = unsafe { &*mapped_ptr };
                    for val in vec {
                        crate::vm::gc::mark_value(val);
                    }
                }));
            });
            for (i, item) in items.iter().enumerate() {
                let res = vm.call_function_reentrant(cb, vec![*item, Value::number(i as f64), receiver])?;
                mapped.push(res);
            }
            crate::vm::gc::GC_ROOTS.with(|roots| {
                roots.borrow_mut().pop();
            });
            let mut out = Vec::new();
            for val in mapped {
                if val.is_array() {
                    unsafe {
                        if let GcData::Array(sub) = &(*val.as_gc_ptr()).data {
                            out.extend_from_slice(sub);
                            continue;
                        }
                    }
                }
                out.push(val);
            }
            let ptr = gc_alloc_array(&out);
            Ok(Value::array(ptr))
        }
        _ => Err("Invalid array iter method".to_string()),
    }
}
