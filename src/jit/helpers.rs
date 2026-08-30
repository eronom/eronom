use std::time::Instant;
use std::rc::Rc;
use crate::vm::execute::{VM, format_undeclared_var_error};
use crate::vm::value::{Value, MapKey};
use crate::vm::gc::{gc_allocate, gc_alloc_string, gc_write_barrier, GcData, get_or_create_string, get_pooled_vec, get_pooled_map, GC_NEEDS_STEP};
use super::profile::{JIT_PROFILING, JIT_PROFILER};
use fnv::FnvHashMap;

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
                GcData::String(s) => s.chars().map(|c| {
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

#[derive(Clone, Copy)]
struct GlobalIcEntry {
    key: u64,
    val: Value,
}

thread_local! {
    static GLOBAL_IC: std::cell::UnsafeCell<[GlobalIcEntry; 64]> = const { std::cell::UnsafeCell::new([GlobalIcEntry { key: 0, val: Value::null() }; 64]) };
}

pub fn reset_global_ic() {
    GLOBAL_IC.with(|c| unsafe {
        *c.get() = [GlobalIcEntry { key: 0, val: Value::null() }; 64];
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_define_global(vm: *mut VM, name_val: Value, val: Value) -> i64 {
    unsafe {
        let name_str = name_val.as_str().unwrap_or("");
        let name: Rc<str> = Rc::from(name_str);
        let key = name_val.0;
        let slot = (key ^ (key >> 6)) as usize & 63;
        let ic = &mut (*GLOBAL_IC.with(|c| c.get()))[slot];
        ic.key = key;
        ic.val = val;
        (*vm).globals.insert(name, val);
        0
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_get_global(vm: *mut VM, name_val: Value) -> Value {
    unsafe {
        let key = name_val.0;
        let slot = (key ^ (key >> 6)) as usize & 63;
        let ic = &mut (*GLOBAL_IC.with(|c| c.get()))[slot];
        if ic.key == key {
            return ic.val;
        }

        let name = name_val.as_str().unwrap_or("");
        if let Some(val) = (*vm).globals.get(name) {
            ic.key = key;
            ic.val = *val;
            *val
        } else {
            (*vm).has_error_flag = 1; (*vm).error = Some(format!("Undefined variable '{}'", name));
            Value::null()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_set_global(vm: *mut VM, val: Value, name_val: Value) -> i64 {
    unsafe {
        let name = name_val.as_str().unwrap_or("");
        match (*vm).globals.get_mut(name) {
            Some(entry) => {
                let key = name_val.0;
                let slot = (key ^ (key >> 6)) as usize & 63;
                let ic = &mut (*GLOBAL_IC.with(|c| c.get()))[slot];
                ic.key = key;
                ic.val = val;
                *entry = val;
                0
            }
            None => {
                (*vm).has_error_flag = 1; (*vm).error = Some(format_undeclared_var_error(name));
                -1
            }
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_has_error(vm: *mut VM) -> i64 {
    // Read the fast inline flag (always in sync with vm.error)
    unsafe { (*vm).has_error_flag as i64 }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_needs_gc() -> i64 {
    if GC_NEEDS_STEP.load(std::sync::atomic::Ordering::Relaxed) { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_make_array(_vm: *mut VM, start_reg: *const Value, count: i64) -> Value {
    let start_time = if JIT_PROFILING { Some(Instant::now()) } else { None };
    unsafe {
        let count_usize = count as usize;
        let slice = std::slice::from_raw_parts(start_reg, count_usize);
        let ptr = crate::vm::gc::gc_alloc_array(slice);
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
pub extern "C" fn er_jit_define_struct(vm: *mut VM, name_val: Value, fields_val: Value, methods_val: Value) -> i64 {
    unsafe {
        let name_rc: Rc<str> = Rc::from(name_val.as_str().unwrap_or(""));
        let fields_vec = match &(*fields_val.as_gc_ptr()).data {
            GcData::Array(arr) => arr,
            _ => return -1,
        };
        
        let mut field_indices = FnvHashMap::default();
        for (idx, &f_val) in fields_vec.iter().enumerate() {
            field_indices.insert(MapKey(f_val), idx);
        }

        let mut methods = FnvHashMap::default();
        if methods_val.is_object() {
            let methods_ptr = methods_val.as_gc_ptr();
            if let GcData::Object(map) = &(*methods_ptr).data {
                for (k, &v) in map {
                    methods.insert(*k, v);
                }
            }
        }
        
        let descriptor = std::rc::Rc::new(crate::vm::gc::StructDescriptor::new(
            name_rc.clone(),
            field_indices,
            methods,
        ));
        (*vm).structs.insert(name_rc.clone(), descriptor.clone());
        let ptr = gc_allocate(GcData::StructConstructor(descriptor));
        (*vm).globals.insert(name_rc, Value::object(ptr));
        0
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_make_object(vm: *mut VM, start_reg: *const Value, count: i64) -> Value {
    let start_time = if JIT_PROFILING { Some(Instant::now()) } else { None };
    unsafe {
        let count_usize = count as usize;
        let ptr = if count_usize == 0 {
            let obj = get_pooled_map(0);
            gc_allocate(GcData::Object(obj))
        } else if count_usize == 2 {
            let k0 = *start_reg;
            let v0 = *start_reg.add(1);
            let k1 = *start_reg.add(2);
            let v1 = *start_reg.add(3);
            if !k0.is_string() || !k1.is_string() {
                (*vm).has_error_flag = 1;
                (*vm).error = Some("Object key must be string".into());
                return Value::null();
            }
            let keys = [k0, k1];
            if let Some((desc, offsets)) = (*vm).find_matching_struct_cached(&keys) {
                let mut fields = crate::vm::gc::get_pooled_vec(2);
                fields.clear();
                if offsets[0] == 0 {
                    fields.push(v0);
                    fields.push(v1);
                } else {
                    fields.push(v1);
                    fields.push(v0);
                }
                gc_allocate(GcData::Struct(crate::vm::gc::GcStruct {
                    descriptor: desc,
                    fields,
                }))
            } else {
                let (desc, offsets) = crate::vm::shape::get_or_create_anonymous_shape_2(k0, k1);
                let mut fields = crate::vm::gc::get_pooled_vec(2);
                fields.clear();
                if offsets[0] == 0 {
                    fields.push(v0);
                    fields.push(v1);
                } else {
                    fields.push(v1);
                    fields.push(v0);
                }
                gc_allocate(GcData::Struct(crate::vm::gc::GcStruct {
                    descriptor: desc,
                    fields,
                }))
            }
        } else if count_usize <= 16 {
            let mut keys = [Value::null(); 16];
            let mut values = [Value::null(); 16];
            for i in 0..count_usize {
                let key_val = *start_reg.offset((i * 2) as isize);
                let val = *start_reg.offset((i * 2 + 1) as isize);
                if !key_val.is_string() {
                    (*vm).has_error_flag = 1;
                    (*vm).error = Some("Object key must be string".into());
                    return Value::null();
                }
                keys[i] = key_val;
                values[i] = val;
            }

            if let Some((desc, offsets)) = (*vm).find_matching_struct_cached(&keys[..count_usize]) {
                let mut fields = crate::vm::gc::get_pooled_vec(count_usize);
                fields.clear();
                fields.resize(count_usize, Value::null());
                for i in 0..count_usize {
                    let val = values[i];
                    let idx = offsets[i];
                    fields[idx] = val;
                }
                gc_allocate(GcData::Struct(crate::vm::gc::GcStruct {
                    descriptor: desc,
                    fields,
                }))
            } else {
                let (desc, offsets) = crate::vm::shape::get_or_create_anonymous_shape(&keys[..count_usize]);
                let mut fields = crate::vm::gc::get_pooled_vec(count_usize);
                fields.clear();
                fields.resize(count_usize, Value::null());
                for i in 0..count_usize {
                    fields[offsets[i]] = values[i];
                }
                gc_allocate(GcData::Struct(crate::vm::gc::GcStruct {
                    descriptor: desc,
                    fields,
                }))
            }
        } else {
            let mut keys = Vec::with_capacity(count_usize);
            let mut values = Vec::with_capacity(count_usize);
            for i in 0..count_usize {
                let key_val = *start_reg.offset((i * 2) as isize);
                let val = *start_reg.offset((i * 2 + 1) as isize);
                if !key_val.is_string() {
                    (*vm).has_error_flag = 1;
                    (*vm).error = Some("Object key must be string".into());
                    return Value::null();
                }
                keys.push(key_val);
                values.push(val);
            }

            if let Some((desc, offsets)) = (*vm).find_matching_struct_cached(&keys) {
                let mut fields = crate::vm::gc::get_pooled_vec(keys.len());
                fields.resize(keys.len(), Value::null());
                for i in 0..count_usize {
                    let val = values[i];
                    let idx = offsets[i];
                    fields[idx] = val;
                }
                gc_allocate(GcData::Struct(crate::vm::gc::GcStruct {
                    descriptor: desc,
                    fields,
                }))
            } else {
                let (desc, offsets) = crate::vm::shape::get_or_create_anonymous_shape(&keys);
                let mut fields = crate::vm::gc::get_pooled_vec(keys.len());
                fields.resize(keys.len(), Value::null());
                for i in 0..count_usize {
                    fields[offsets[i]] = values[i];
                }
                gc_allocate(GcData::Struct(crate::vm::gc::GcStruct {
                    descriptor: desc,
                    fields,
                }))
            }
        };
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

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_call_fast(
    vm: *mut VM,
    callee: Value,
    callee_frame_slots: *mut Value,
    dest: *mut Value,
    inst_idx: i64,
    dest_reg: i64,
) -> i64 {
    unsafe {
        if let Some(frame) = (*vm).frames.last_mut() {
            frame.ip = inst_idx as usize;
        }
        if !callee.is_function() {
            return -1;
        }
        let func_ptr = (callee.0 & crate::vm::value::PTR_MASK) as *mut crate::vm::gc::GcObject;
        let (raw_fn_ptr, func) = match &(*func_ptr).data {
            GcData::Function(f) => (func_ptr, f),
            GcData::Closure(c) => match &(*c.function).data {
                GcData::Function(f) => (c.function, f),
                _ => return -1,
            },
            _ => return -1,
        };

        if func.is_async || !func.chunk.handlers.is_empty() {
            return -1;
        }

        let count = func.invocation_count.get() + 1;
        func.invocation_count.set(count);

        let native_ptr = if let Some(ptr) = func.jit_ptr.get() {
            ptr
        } else if (*vm).jit_threshold == 0 || func.has_loop || count >= (*vm).jit_threshold {
            crate::jit::compile_function(&mut *vm, raw_fn_ptr)
        } else {
            return -1;
        };

        type JitFn = unsafe extern "C" fn(
            vm: *mut VM,
            frame_slots: *mut Value,
            constants_ptr: *const Value,
            start_ip: usize,
            ip_out: *mut usize,
            dest_reg_out: *mut usize,
            func_reg_out: *mut usize,
            arg_count_out: *mut usize,
            ret_val_out: *mut Value,
        ) -> i64;

        let jit_fn: JitFn = std::mem::transmute(native_ptr);
        let constants_ptr = func.chunk.constants.as_ptr();

        let mut ip_out: usize = 0;
        let mut dest_reg_out: usize = 0;
        let mut func_reg_out: usize = 0;
        let mut arg_count_out: usize = 0;
        let mut ret_val_out: Value = Value::null();

        let slots_offset = callee_frame_slots.offset_from((*vm).stack.as_ptr()) as usize;
        (*vm).frames.push(crate::vm::execute::CallFrame {
            function: func_ptr,
            ip: 0,
            slots_offset,
            dest_reg: dest_reg as usize,
        });

        let res = jit_fn(
            vm,
            callee_frame_slots,
            constants_ptr,
            0,
            &mut ip_out,
            &mut dest_reg_out,
            &mut func_reg_out,
            &mut arg_count_out,
            &mut ret_val_out,
        );

        if res == 1 {
            if !(*vm).stack.is_empty() {
                (*vm).close_upvalues(slots_offset);
                (*vm).frames.pop();
            }
            *dest = ret_val_out;
            0
        } else if res == 3 {
            -3
        } else if res == 4 {
            let initial_depth = (*vm).frames.len() - 1;
            match (*vm).execute_loop_interpreter(initial_depth) {
                Ok(val) => {
                    *dest = val;
                    0
                }
                Err(e) => {
                    (*vm).error = Some(e);
                    -2
                }
            }
        } else {
            if !ret_val_out.is_null() {
                (*vm).thrown_value = ret_val_out;
            }
            -2
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
    inst_idx: i64,
    dest_reg: i64,
) -> i64 {
    let start_time = if JIT_PROFILING { Some(Instant::now()) } else { None };
    unsafe {
        if let Some(frame) = (*_vm).frames.last_mut() {
            frame.ip = inst_idx as usize;
        }
        let status = if callee.is_function() {
            let mut func_ptr = callee.as_gc_ptr();
            if let GcData::BuiltinMethod(builtin) = &(*func_ptr).data {
                let receiver = builtin.receiver;
                let method = builtin.method;
                let args = std::slice::from_raw_parts(frame_slots.offset((func_reg + 1) as isize), arg_count as usize);
                match (*_vm).execute_builtin_method(receiver, method, args) {
                    Ok(res) => {
                        *dest = res;
                        0
                    }
                    Err(err) => {
                        (*_vm).error = Some(err);
                        (*_vm).has_error_flag = 1;
                        -2
                    }
                }
            } else {
                let mut actual_arg_count = arg_count as usize;
                let mut callee_frame_slots = frame_slots.offset((func_reg + 1) as isize);

                if let GcData::BoundMethod(bound_method) = &(*func_ptr).data {
                    for i in (0..actual_arg_count).rev() {
                        *frame_slots.offset((func_reg + 2 + i as i64) as isize) = *frame_slots.offset((func_reg + 1 + i as i64) as isize);
                    }
                    *frame_slots.offset((func_reg + 1) as isize) = bound_method.receiver;
                    func_ptr = bound_method.function;
                    actual_arg_count += 1;
                }

                let raw_fn_ptr = match &(*func_ptr).data {
                    GcData::Function(_) => func_ptr,
                    GcData::Closure(c) => c.function,
                    _ => return -1,
                };

            let func_val = match &(*raw_fn_ptr).data {
                GcData::Function(func) => func,
                _ => return -1,
            };

            if actual_arg_count < func_val.arity {
                for i in actual_arg_count..func_val.arity {
                    *callee_frame_slots.add(i) = Value::null();
                }
            } else if actual_arg_count > func_val.arity {
                (*_vm).error = Some(format!(
                    "Expected {} args but got {}",
                    func_val.arity, actual_arg_count
                ));
                (*_vm).has_error_flag = 1;
                return -2;
            }

            if func_val.is_async || !func_val.chunk.handlers.is_empty() {
                return -1; // Fallback to host VM loop for async or exception handlers
            } else {
                let offset_from_base = callee_frame_slots.offset_from((*_vm).stack.as_ptr()) as usize;
                if offset_from_base + 512 >= (*_vm).stack.len() {
                    let new_len = (*_vm).stack.len() * 2;
                    (*_vm).stack.resize(new_len, Value::null());
                    callee_frame_slots = (*_vm).stack.as_mut_ptr().add(offset_from_base);
                }

                let count = func_val.invocation_count.get() + 1;
                func_val.invocation_count.set(count);

                let native_ptr = if let Some(ptr) = func_val.jit_ptr.get() {
                    ptr
                } else if (*_vm).jit_threshold == 0 || func_val.has_loop || count >= (*_vm).jit_threshold {
                    crate::jit::compile_function(&mut *_vm, raw_fn_ptr)
                } else {
                    return -1;
                };

                type JitFn = unsafe extern "C" fn(
                    vm: *mut VM,
                    frame_slots: *mut Value,
                    constants_ptr: *const Value,
                    start_ip: usize,
                    ip_out: *mut usize,
                    dest_reg_out: *mut usize,
                    func_reg_out: *mut usize,
                    arg_count_out: *mut usize,
                    ret_val_out: *mut Value,
                ) -> i64;

                let jit_fn: JitFn = std::mem::transmute(native_ptr);
                let constants_ptr = func_val.chunk.constants.as_ptr();

                let mut ip_out: usize = 0;
                let mut dest_reg_out: usize = 0;
                let mut func_reg_out: usize = 0;
                let mut arg_count_out: usize = 0;
                let mut ret_val_out: Value = Value::null();

                let slots_offset = callee_frame_slots.offset_from((*_vm).stack.as_ptr()) as usize;
                (*_vm).frames.push(crate::vm::execute::CallFrame {
                    function: func_ptr,
                    ip: 0,
                    slots_offset,
                    dest_reg: dest_reg as usize,
                });

                let jit_res = jit_fn(
                    _vm,
                    callee_frame_slots,
                    constants_ptr,
                    0,
                    &mut ip_out,
                    &mut dest_reg_out,
                    &mut func_reg_out,
                    &mut arg_count_out,
                    &mut ret_val_out,
                );

                if jit_res == 1 {
                    if !(*_vm).stack.is_empty() {
                        (*_vm).close_upvalues(slots_offset);
                        (*_vm).frames.pop();
                    }
                    *dest = ret_val_out;
                    0
                } else if jit_res == 3 {
                    -3
                } else if jit_res == 4 {
                    let initial_depth = (*_vm).frames.len() - 1;
                    match (*_vm).execute_loop_interpreter(initial_depth) {
                        Ok(val) => {
                            *dest = val;
                            0
                        }
                        Err(e) => {
                            (*_vm).error = Some(e);
                            -2
                        }
                    }
                } else {
                    if !ret_val_out.is_null() {
                        (*_vm).thrown_value = ret_val_out;
                    }
                    -2
                }
            }
            }
        } else if callee.is_native_function() {
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
        } else if callee.is_object() && matches!(&(*callee.as_gc_ptr()).data, GcData::StructConstructor(_)) {
            let ptr = callee.as_gc_ptr();
            let descriptor = match &(*ptr).data {
                GcData::StructConstructor(desc) => desc.clone(),
                _ => return -1,
            };
            let mut args = Vec::with_capacity(arg_count as usize);
            for i in 0..arg_count {
                args.push(*frame_slots.offset((func_reg + 1 + i) as isize));
            }
            match construct_struct_from_args_helper(&descriptor, args) {
                Ok(result) => {
                    *dest = result;
                    0
                }
                Err(err) => {
                    (*_vm).error = Some(err);
                    -2
                }
            }
        } else if callee.is_method_json() || callee.is_method_text() {
            let ptr = callee.as_gc_ptr();
            let result = match &(*ptr).data {
                GcData::Object(map) => {
                    let body_key = get_or_create_string("_body");
                    let body_val = map.get(&MapKey(Value::string(body_key))).cloned().unwrap_or(Value::null());
                    if callee.is_method_json() {
                        if body_val.is_string() {
                            let s = body_val.as_str().unwrap_or("");
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
                _ => Value::null(),
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
                _ => Value::null(),
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
            _ => Value::null(),
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
            _ => Value::null(),
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
pub extern "C" fn er_jit_get_upvalue(vm: *mut VM, upval_idx: i64) -> Value {
    unsafe {
        let frame = (*vm).frames.last().unwrap();
        let upval_ptr = match &(*frame.function).data {
            GcData::Closure(c) => c.upvalues[upval_idx as usize],
            _ => return Value::null(),
        };
        let val = match &(*upval_ptr).data {
            GcData::Upvalue(u) => match u.location {
                crate::vm::gc::UpvalueLocation::Open(slot) => (*vm).stack.as_ptr().add(slot).read(),
                crate::vm::gc::UpvalueLocation::Closed(val) => val,
            },
            _ => Value::null(),
        };
        val
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_set_upvalue(vm: *mut VM, upval_idx: i64, val: Value) -> i64 {
    unsafe {
        let frame = (*vm).frames.last().unwrap();
        let upval_ptr = match &(*frame.function).data {
            GcData::Closure(c) => c.upvalues[upval_idx as usize],
            _ => return -1,
        };
        match &mut (*upval_ptr).data {
            GcData::Upvalue(u) => match u.location {
                crate::vm::gc::UpvalueLocation::Open(slot) => {
                    (*vm).stack.as_mut_ptr().add(slot).write(val);
                }
                crate::vm::gc::UpvalueLocation::Closed(ref mut v) => {
                    *v = val;
                }
            },
            _ => return -1,
        }
        0
    }
}


#[unsafe(no_mangle)]
pub extern "C" fn er_jit_make_closure(vm: *mut VM, raw_fn_val: Value) -> Value {
    unsafe {
        let raw_fn_ptr = raw_fn_val.as_gc_ptr();
        let fn_proto = match &(*raw_fn_ptr).data {
            GcData::Function(f) => f,
            _ => return Value::null(),
        };
        let frame = (*vm).frames.last().unwrap();
        let slots_offset = frame.slots_offset;
        let mut upvalue_ptrs = Vec::with_capacity(fn_proto.upvalues.len());
        for uv_desc in &fn_proto.upvalues {
            if uv_desc.is_local {
                let abs_slot = slots_offset + uv_desc.index as usize;
                let uv_ptr = (*vm).capture_upvalue(abs_slot);
                upvalue_ptrs.push(uv_ptr);
            } else {
                let parent_uv_ptr = match &(*frame.function).data {
                    GcData::Closure(c) => c.upvalues[uv_desc.index as usize],
                    _ => return Value::null(),
                };
                upvalue_ptrs.push(parent_uv_ptr);
            }
        }
        let closure_ptr = gc_allocate(GcData::Closure(crate::vm::gc::GcClosure {
            function: raw_fn_ptr,
            upvalues: upvalue_ptrs,
        }));
        Value::function(closure_ptr)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_close_upvalues(vm: *mut VM, rel_slot: i64) -> i64 {
    unsafe {
        if !(*vm).open_upvalues.is_empty() {
            if let Some(frame) = (*vm).frames.last() {
                let slot = frame.slots_offset + rel_slot as usize;
                (*vm).close_upvalues(slot);
            }
        }
        0
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_await(vm: *mut VM, await_val: Value, dest: *mut Value) -> i64 {
    unsafe {
        if await_val.is_promise() {
            let promise_ptr = await_val.as_gc_ptr();
            let state = match &(*promise_ptr).data {
                crate::vm::gc::GcData::Promise(prom) => prom.state.clone(),
                _ => return -1,
            };
            let promise_status = {
                let lock = state.lock().unwrap();
                lock.clone()
            };
            match promise_status {
                crate::vm::gc::PromiseState::Fulfilled(val) => {
                    *dest = val;
                    0
                }
                crate::vm::gc::PromiseState::Rejected(err) => {
                    (*vm).has_error_flag = 1; (*vm).error = Some(err);
                    -1
                }
                crate::vm::gc::PromiseState::Pending => {
                    // Pending promise: Needs suspend
                    -3
                }
            }
        } else {
            *dest = await_val;
            0
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_reset_context() {
    crate::jit::compiler::reset_jit_state();
}

#[unsafe(no_mangle)]
pub extern "C" fn er_jit_write_barrier(parent: *mut crate::vm::gc::GcObject, child: Value) {
    gc_write_barrier(parent, &child);
}

