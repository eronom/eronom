use crate::vm::value::Value;
use crate::vm::gc::{gc_alloc_array, gc_alloc_string, gc_write_barrier, BuiltinMethodId, GcData};
use crate::vm::execute::types::VM;

pub fn execute_array_core_method(
    _vm: &mut VM,
    receiver: Value,
    method: BuiltinMethodId,
    args: &[Value],
) -> Result<Value, String> {
    use BuiltinMethodId::*;
    match method {
        ArrayPush => {
            let ptr = receiver.as_gc_ptr();
            unsafe {
                match &mut (*ptr).data {
                    GcData::Array(arr) => {
                        for &arg in args {
                            gc_write_barrier(ptr, &arg);
                            arr.push(arg);
                        }
                        Ok(Value::number(arr.len() as f64))
                    }
                    _ => Ok(Value::null()),
                }
            }
        }
        ArrayPop => {
            let ptr = receiver.as_gc_ptr();
            unsafe {
                match &mut (*ptr).data {
                    GcData::Array(arr) => {
                        Ok(arr.pop().unwrap_or(Value::null()))
                    }
                    _ => Ok(Value::null()),
                }
            }
        }
        ArrayShift => {
            let ptr = receiver.as_gc_ptr();
            unsafe {
                match &mut (*ptr).data {
                    GcData::Array(arr) => {
                        if arr.is_empty() {
                            Ok(Value::null())
                        } else {
                            Ok(arr.remove(0))
                        }
                    }
                    _ => Ok(Value::null()),
                }
            }
        }
        ArrayUnshift => {
            let ptr = receiver.as_gc_ptr();
            unsafe {
                match &mut (*ptr).data {
                    GcData::Array(arr) => {
                        arr.splice(0..0, args.iter().copied());
                        for arg in args {
                            gc_write_barrier(ptr, arg);
                        }
                        Ok(Value::number(arr.len() as f64))
                    }
                    _ => Ok(Value::null()),
                }
            }
        }
        ArrayIncludes => {
            let search = args.get(0).copied().unwrap_or(Value::null());
            let from_idx = args.get(1).map(|v| if v.is_number() { (v.as_number() as isize).max(0) as usize } else { 0 }).unwrap_or(0);
            let ptr = receiver.as_gc_ptr();
            let mut found = false;
            unsafe {
                if let GcData::Array(arr) = &(*ptr).data {
                    if from_idx < arr.len() {
                        for item in &arr[from_idx..] {
                            if *item == search {
                                found = true;
                                break;
                            }
                        }
                    }
                }
            }
            Ok(Value::boolean(found))
        }
        ArrayIndexOf => {
            let search = args.get(0).copied().unwrap_or(Value::null());
            let from_idx = args.get(1).map(|v| if v.is_number() { (v.as_number() as isize).max(0) as usize } else { 0 }).unwrap_or(0);
            let ptr = receiver.as_gc_ptr();
            let mut found_idx = -1.0;
            unsafe {
                if let GcData::Array(arr) = &(*ptr).data {
                    if from_idx < arr.len() {
                        for (i, item) in arr.iter().enumerate().skip(from_idx) {
                            if *item == search {
                                found_idx = i as f64;
                                break;
                            }
                        }
                    }
                }
            }
            Ok(Value::number(found_idx))
        }
        ArrayLastIndexOf => {
            let search = args.get(0).copied().unwrap_or(Value::null());
            let ptr = receiver.as_gc_ptr();
            let mut found_idx = -1.0;
            unsafe {
                if let GcData::Array(arr) = &(*ptr).data {
                    let from_idx = args.get(1).map(|v| if v.is_number() { (v.as_number() as usize).min(arr.len().saturating_sub(1)) } else { arr.len().saturating_sub(1) }).unwrap_or_else(|| arr.len().saturating_sub(1));
                    if !arr.is_empty() && from_idx < arr.len() {
                        for (i, item) in arr[..=from_idx].iter().enumerate().rev() {
                            if *item == search {
                                found_idx = i as f64;
                                break;
                            }
                        }
                    }
                }
            }
            Ok(Value::number(found_idx))
        }
        ArraySlice => {
            let ptr = receiver.as_gc_ptr();
            let sub_vec = unsafe {
                match &(*ptr).data {
                    GcData::Array(arr) => {
                        let len = arr.len() as isize;
                        let start = args.get(0).map(|v| if v.is_number() { v.as_number() as isize } else { 0 }).unwrap_or(0);
                        let end = args.get(1).map(|v| if v.is_number() { v.as_number() as isize } else { len }).unwrap_or(len);
                        let start_idx = if start < 0 { (len + start).max(0) as usize } else { (start as usize).min(arr.len()) };
                        let end_idx = if end < 0 { (len + end).max(0) as usize } else { (end as usize).min(arr.len()) };
                        if start_idx >= end_idx {
                            vec![]
                        } else {
                            arr[start_idx..end_idx].to_vec()
                        }
                    }
                    _ => vec![],
                }
            };
            let ptr = gc_alloc_array(&sub_vec);
            Ok(Value::array(ptr))
        }
        ArrayJoin => {
            let sep = args.get(0).and_then(|v| v.as_str()).unwrap_or(",");
            let ptr = receiver.as_gc_ptr();
            let mut s = String::new();
            unsafe {
                if let GcData::Array(arr) = &(*ptr).data {
                    for (i, item) in arr.iter().enumerate() {
                        if i > 0 { s.push_str(sep); }
                        if let Some(str_val) = item.as_str() {
                            s.push_str(str_val);
                        } else {
                            s.push_str(&item.to_string());
                        }
                    }
                }
            }
            let ptr = gc_alloc_string(&s);
            Ok(Value::string(ptr))
        }
        ArrayConcat => {
            let ptr = receiver.as_gc_ptr();
            let mut combined = unsafe {
                match &(*ptr).data {
                    GcData::Array(arr) => arr.clone(),
                    _ => vec![],
                }
            };
            for arg in args {
                if arg.is_array() {
                    let sub_ptr = arg.as_gc_ptr();
                    unsafe {
                        if let GcData::Array(sub_arr) = &(*sub_ptr).data {
                            combined.extend_from_slice(sub_arr);
                        }
                    }
                } else {
                    combined.push(*arg);
                }
            }
            let ptr = gc_alloc_array(&combined);
            Ok(Value::array(ptr))
        }
        ArrayReverse => {
            let ptr = receiver.as_gc_ptr();
            unsafe {
                match &mut (*ptr).data {
                    GcData::Array(arr) => {
                        arr.reverse();
                    }
                    _ => {}
                }
            }
            Ok(receiver)
        }
        ArraySort => {
            let cb_opt = args.get(0).copied();
            let ptr = receiver.as_gc_ptr();
            let mut items: Vec<Value> = unsafe {
                match &(*ptr).data {
                    GcData::Array(arr) => arr.clone(),
                    _ => vec![],
                }
            };
            if let Some(cb) = cb_opt {
                if cb.is_function() || cb.is_native_function() {
                    let len = items.len();
                    for i in 0..len {
                        for j in 0..len.saturating_sub(1 + i) {
                            let cmp_res = _vm.call_function_reentrant(cb, vec![items[j], items[j + 1]])?;
                            let cmp_val = cmp_res.as_number();
                            if cmp_val > 0.0 {
                                items.swap(j, j + 1);
                            }
                        }
                    }
                } else {
                    items.sort_by(|a, b| {
                        if a.is_number() && b.is_number() {
                            a.as_number().partial_cmp(&b.as_number()).unwrap_or(std::cmp::Ordering::Equal)
                        } else {
                            a.to_string().cmp(&b.to_string())
                        }
                    });
                }
            } else {
                items.sort_by(|a, b| {
                    if a.is_number() && b.is_number() {
                        a.as_number().partial_cmp(&b.as_number()).unwrap_or(std::cmp::Ordering::Equal)
                    } else {
                        a.to_string().cmp(&b.to_string())
                    }
                });
            }
            unsafe {
                match &mut (*ptr).data {
                    GcData::Array(arr) => {
                        arr.clear();
                        arr.extend_from_slice(&items);
                    }
                    _ => {}
                }
            }
            Ok(receiver)
        }
        ArrayFlat => {
            let depth = args.get(0).map(|v| if v.is_number() { (v.as_number() as usize).max(0) } else { 1 }).unwrap_or(1);
            let items: Vec<Value> = unsafe {
                match &(*receiver.as_gc_ptr()).data {
                    GcData::Array(arr) => arr.clone(),
                    _ => vec![],
                }
            };
            fn flatten_helper(val: Value, depth: usize, out: &mut Vec<Value>) {
                if depth > 0 && val.is_array() {
                    unsafe {
                        if let GcData::Array(sub) = &(*val.as_gc_ptr()).data {
                            for item in sub {
                                flatten_helper(*item, depth - 1, out);
                            }
                            return;
                        }
                    }
                }
                out.push(val);
            }
            let mut out = Vec::new();
            for item in items {
                flatten_helper(item, depth, &mut out);
            }
            let ptr = gc_alloc_array(&out);
            Ok(Value::array(ptr))
        }
        ArrayFill => {
            let val = args.get(0).copied().unwrap_or(Value::null());
            let ptr = receiver.as_gc_ptr();
            unsafe {
                match &mut (*ptr).data {
                    GcData::Array(arr) => {
                        let len = arr.len() as isize;
                        let start = args.get(1).map(|v| if v.is_number() { v.as_number() as isize } else { 0 }).unwrap_or(0);
                        let end = args.get(2).map(|v| if v.is_number() { v.as_number() as isize } else { len }).unwrap_or(len);
                        let start_idx = if start < 0 { (len + start).max(0) as usize } else { (start as usize).min(arr.len()) };
                        let end_idx = if end < 0 { (len + end).max(0) as usize } else { (end as usize).min(arr.len()) };
                        for i in start_idx..end_idx {
                            arr[i] = val;
                            gc_write_barrier(ptr, &val);
                        }
                    }
                    _ => {}
                }
            }
            Ok(receiver)
        }
        _ => Err("Invalid array core method".to_string()),
    }
}
