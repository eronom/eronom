use std::time::Instant;
use crate::vm::execute::VM;
use crate::vm::value::Value;
use crate::vm::gc::{gc_allocate, gc_alloc_string, GcData, get_or_create_string, GC_NEEDS_STEP};
use crate::jit::profile::{JIT_PROFILING, JIT_PROFILER};

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_negate(vm: *mut VM, val: Value) -> Value {
    unsafe {
        if val.is_number() {
            Value::number_unchecked(-val.as_number())
        } else {
            (*vm).has_error_flag = 1; (*vm).error = Some("Operand must be a number".into());
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
        } else if val_b.is_string() {
            let sa_str = val_b.as_str().unwrap_or("");
            let mut buf = [0u8; 64];
            let sa_bytes = sa_str.as_bytes();
            let mut len = sa_bytes.len().min(63);
            buf[..len].copy_from_slice(&sa_bytes[..len]);
            if val_c.is_string() {
                if let Some(sb_str) = val_c.as_str() {
                    let sb_bytes = sb_str.as_bytes();
                    let to_copy = sb_bytes.len().min(63 - len);
                    buf[len..len + to_copy].copy_from_slice(&sb_bytes[..to_copy]);
                    len += to_copy;
                }
            } else if val_c.is_number() {
                let val = val_c.as_number();
                if val >= 0.0 && val == val.trunc() && val < 1e15 {
                    let mut n = val as u64;
                    if n == 0 {
                        if len < 64 { buf[len] = b'0'; len += 1; }
                    } else {
                        let mut digits = [0u8; 20];
                        let mut d_len = 0;
                        while n > 0 {
                            digits[d_len] = b'0' + (n % 10) as u8;
                            n /= 10;
                            d_len += 1;
                        }
                        for i in (0..d_len).rev() {
                            if len < 64 {
                                buf[len] = digits[i];
                                len += 1;
                            }
                        }
                    }
                } else {
                    use std::io::Write;
                    let _ = write!(&mut buf[len..], "{}", val);
                }
            } else {
                use std::io::Write;
                let _ = write!(&mut buf[len..], "{}", val_c);
            }
            let s = std::str::from_utf8_unchecked(&buf[..len]);
            if let Some(inline) = Value::inline_string(s) {
                inline
            } else {
                let new_ptr = gc_alloc_string(s);
                Value::string(new_ptr)
            }
        } else if val_c.is_string() {
            let sb_str = val_c.as_str().unwrap_or("");
            let mut buf = [0u8; 64];
            let mut len = 0;
            if val_b.is_number() {
                let val = val_b.as_number();
                if val >= 0.0 && val == val.trunc() && val < 1e15 {
                    let mut n = val as u64;
                    if n == 0 {
                        if len < 64 { buf[len] = b'0'; len += 1; }
                    } else {
                        let mut digits = [0u8; 20];
                        let mut d_len = 0;
                        while n > 0 {
                            digits[d_len] = b'0' + (n % 10) as u8;
                            n /= 10;
                            d_len += 1;
                        }
                        for i in (0..d_len).rev() {
                            if len < 64 {
                                buf[len] = digits[i];
                                len += 1;
                            }
                        }
                    }
                } else {
                    use std::io::Write;
                    let _ = write!(&mut buf[len..], "{}", val);
                }
            } else {
                use std::io::Write;
                let _ = write!(&mut buf[len..], "{}", val_b);
            }
            let sb_bytes = sb_str.as_bytes();
            let to_copy = sb_bytes.len().min(63 - len);
            buf[len..len + to_copy].copy_from_slice(&sb_bytes[..to_copy]);
            len += to_copy;
            let s = std::str::from_utf8_unchecked(&buf[..len]);
            if let Some(inline) = Value::inline_string(s) {
                inline
            } else {
                let new_ptr = gc_alloc_string(s);
                Value::string(new_ptr)
            }
        } else {
            (*vm).has_error_flag = 1; (*vm).error = Some("Operands must be numbers or strings".into());
            Value::null()
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
            (*vm).has_error_flag = 1; (*vm).error = Some("Operands must be numbers".into());
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
            (*vm).has_error_flag = 1; (*vm).error = Some("Operands must be numbers".into());
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
            (*vm).has_error_flag = 1; (*vm).error = Some("Operands must be numbers".into());
            Value::null()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_mod(vm: *mut VM, val_b: Value, val_c: Value) -> Value {
    unsafe {
        if val_b.is_number() && val_c.is_number() {
            Value::number_unchecked(val_b.as_number() % val_c.as_number())
        } else {
            (*vm).has_error_flag = 1; (*vm).error = Some("Operands must be numbers".into());
            Value::null()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_bit_and(vm: *mut VM, val_b: Value, val_c: Value) -> Value {
    unsafe {
        if val_b.is_number() && val_c.is_number() {
            let res = ((val_b.as_number() as i64) & (val_c.as_number() as i64)) as f64;
            Value::number_unchecked(res)
        } else {
            (*vm).has_error_flag = 1; (*vm).error = Some("Operands must be numbers".into());
            Value::null()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_bit_or(vm: *mut VM, val_b: Value, val_c: Value) -> Value {
    unsafe {
        if val_b.is_number() && val_c.is_number() {
            let res = ((val_b.as_number() as i64) | (val_c.as_number() as i64)) as f64;
            Value::number_unchecked(res)
        } else {
            (*vm).has_error_flag = 1; (*vm).error = Some("Operands must be numbers".into());
            Value::null()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_bit_xor(vm: *mut VM, val_b: Value, val_c: Value) -> Value {
    unsafe {
        if val_b.is_number() && val_c.is_number() {
            let res = ((val_b.as_number() as i64) ^ (val_c.as_number() as i64)) as f64;
            Value::number_unchecked(res)
        } else {
            (*vm).has_error_flag = 1; (*vm).error = Some("Operands must be numbers".into());
            Value::null()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_bit_not(vm: *mut VM, val: Value) -> Value {
    unsafe {
        if val.is_number() {
            let res = (!(val.as_number() as i64)) as f64;
            Value::number_unchecked(res)
        } else {
            (*vm).has_error_flag = 1; (*vm).error = Some("Operand must be a number".into());
            Value::null()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_shift_left(vm: *mut VM, val_b: Value, val_c: Value) -> Value {
    unsafe {
        if val_b.is_number() && val_c.is_number() {
            let shift = (val_c.as_number() as u32) & 63;
            let res = ((val_b.as_number() as i64).wrapping_shl(shift)) as f64;
            Value::number_unchecked(res)
        } else {
            (*vm).has_error_flag = 1; (*vm).error = Some("Operands must be numbers".into());
            Value::null()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_shift_right(vm: *mut VM, val_b: Value, val_c: Value) -> Value {
    unsafe {
        if val_b.is_number() && val_c.is_number() {
            let shift = (val_c.as_number() as u32) & 63;
            let res = ((val_b.as_number() as i64).wrapping_shr(shift)) as f64;
            Value::number_unchecked(res)
        } else {
            (*vm).has_error_flag = 1; (*vm).error = Some("Operands must be numbers".into());
            Value::null()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_typeof(_vm: *mut VM, val: Value) -> Value {
    let type_str = if val.is_number() {
        "number"
    } else if val.is_string() {
        "string"
    } else if val.is_boolean() {
        "boolean"
    } else if val.is_null() {
        "null"
    } else if val.is_array() {
        "array"
    } else if val.is_object() {
        "object"
    } else if val.is_function() || val.is_native_function() {
        "function"
    } else {
        "object"
    };
    let ptr = get_or_create_string(type_str);
    Value::string(ptr)
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_to_iter(vm: *mut VM, val: Value) -> Value {
    unsafe {
        if val.is_array() {
            val
        } else if val.is_object() {
            let obj_ptr = val.as_gc_ptr();
            let keys: Vec<Value> = match &(*obj_ptr).data {
                GcData::Object(map) => map.keys().map(|k| k.0).collect(),
                GcData::Struct(s) => s.descriptor.field_indices.keys().map(|k| k.0).collect(),
                _ => Vec::new(),
            };
            let arr_ptr = gc_allocate(GcData::Array(keys));
            Value::array(arr_ptr)
        } else if val.is_string() {
            let s_ptr = val.as_gc_ptr();
            let chars: Vec<Value> = match &(*s_ptr).data {
                GcData::String(s) => s.as_str().chars().map(|c| {
                    let cp = gc_alloc_string(&c.to_string());
                    Value::string(cp)
                }).collect(),
                _ => Vec::new(),
            };
            let arr_ptr = gc_allocate(GcData::Array(chars));
            Value::array(arr_ptr)
        } else {
            (*vm).has_error_flag = 1; (*vm).error = Some("Cannot iterate over non-iterable value".into());
            Value::null()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_array_len_op(vm: *mut VM, val: Value) -> Value {
    unsafe {
        if val.is_array() {
            let arr_ptr = val.as_gc_ptr();
            let len = match &(*arr_ptr).data {
                GcData::Array(arr) => arr.len(),
                _ => 0,
            };
            Value::number(len as f64)
        } else {
            (*vm).has_error_flag = 1; (*vm).error = Some("Expected array for length".into());
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
            (*vm).has_error_flag = 1; (*vm).error = Some("Operands must be numbers".into());
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
            (*vm).has_error_flag = 1; (*vm).error = Some("Operands must be numbers".into());
            Value::null()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_has_error(vm: *mut VM) -> i64 {
    unsafe { (*vm).has_error_flag as i64 }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_needs_gc() -> i64 {
    if GC_NEEDS_STEP.load(std::sync::atomic::Ordering::Relaxed) { 1 } else { 0 }
}
