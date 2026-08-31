use std::rc::Rc;
use std::time::Instant;
use fnv::FnvHashMap;
use crate::vm::execute::{VM, format_undeclared_var_error};
use crate::vm::value::{Value, MapKey};
use crate::vm::gc::{gc_allocate, GcData, get_pooled_map};
use crate::jit::profile::{JIT_PROFILING, JIT_PROFILER};

#[derive(Clone, Copy)]
pub struct GlobalIcEntry {
    pub key: u64,
    pub val: Value,
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
