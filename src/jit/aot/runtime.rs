use std::ffi::{c_char, CStr};
use std::rc::Rc;
use fnv::FnvHashMap;
use crate::vm::gc::{
    gc_alloc_string, gc_allocate, gc_clear_string_cache, gc_free_all,
    get_pooled_map, GcData, GcObject, GcStruct, StructDescriptor,
};
use crate::vm::value::Value;

/// Global runtime initialization entry point called at the start of native binaries.
#[unsafe(no_mangle)]
pub extern "C" fn er_runtime_init() {
    gc_clear_string_cache();
}

/// Global runtime teardown entry point called before native binary exit.
#[unsafe(no_mangle)]
pub extern "C" fn er_runtime_cleanup() {
    gc_clear_string_cache();
    gc_free_all();
}

/// Native memory allocation for strings into the garbage-collected heap.
#[unsafe(no_mangle)]
pub extern "C" fn er_alloc_string(ptr: *const u8, len: usize) -> *mut GcObject {
    if ptr.is_null() || len == 0 {
        return gc_alloc_string("");
    }
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    let s = String::from_utf8_lossy(slice);
    gc_alloc_string(&s)
}

/// Native memory allocation for arrays into the garbage-collected heap.
#[unsafe(no_mangle)]
pub extern "C" fn er_alloc_array(capacity: usize) -> *mut GcObject {
    let vec = Vec::with_capacity(capacity);
    gc_allocate(GcData::Array(vec))
}

/// Native memory allocation for objects into the garbage-collected heap.
#[unsafe(no_mangle)]
pub extern "C" fn er_alloc_object() -> *mut GcObject {
    let map = get_pooled_map(16);
    gc_allocate(GcData::Object(map))
}

/// Native memory allocation for structs into the garbage-collected heap.
#[unsafe(no_mangle)]
pub extern "C" fn er_alloc_struct(name_ptr: *const c_char, field_count: usize) -> *mut GcObject {
    let name = if name_ptr.is_null() {
        "AnonymousStruct".to_string()
    } else {
        unsafe { CStr::from_ptr(name_ptr).to_string_lossy().into_owned() }
    };
    let descriptor = Rc::new(StructDescriptor::new(
        name.into(),
        FnvHashMap::default(),
        FnvHashMap::default(),
    ));
    let instance = GcStruct {
        descriptor,
        fields: vec![Value::null(); field_count],
    };
    gc_allocate(GcData::Struct(instance))
}

/// Print string to standard output without newline.
#[unsafe(no_mangle)]
pub extern "C" fn er_print_string(ptr: *const u8, len: usize) {
    if !ptr.is_null() && len > 0 {
        let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
        let s = String::from_utf8_lossy(slice);
        print!("{}", s);
    }
}

/// Print string to standard output with newline.
#[unsafe(no_mangle)]
pub extern "C" fn er_println_string(ptr: *const u8, len: usize) {
    er_print_string(ptr, len);
    println!();
}

/// Print an unboxed 64-bit integer to standard output with newline.
#[unsafe(no_mangle)]
pub extern "C" fn er_println_i64(val: i64) {
    println!("{}", val);
}

/// Print an unboxed 64-bit float to standard output with newline.
#[unsafe(no_mangle)]
pub extern "C" fn er_println_f64(val: f64) {
    println!("{}", val);
}

/// Print a boolean to standard output with newline.
#[unsafe(no_mangle)]
pub extern "C" fn er_println_bool(val: bool) {
    println!("{}", val);
}

/// Print any boxed dynamic value to standard output.
#[unsafe(no_mangle)]
pub extern "C" fn er_print_value(val: Value) {
    print!("{}", val);
}

/// Print any boxed dynamic value with newline to standard output.
#[unsafe(no_mangle)]
pub extern "C" fn er_println_value(val: Value) {
    println!("{}", val);
}

/// Unboxed fast math: double addition.
#[unsafe(no_mangle)]
pub extern "C" fn er_aot_add_f64(a: f64, b: f64) -> f64 {
    a + b
}

/// Unboxed fast math: double subtraction.
#[unsafe(no_mangle)]
pub extern "C" fn er_aot_sub_f64(a: f64, b: f64) -> f64 {
    a - b
}

/// Unboxed fast math: double multiplication.
#[unsafe(no_mangle)]
pub extern "C" fn er_aot_mul_f64(a: f64, b: f64) -> f64 {
    a * b
}

/// Unboxed fast math: double division.
#[unsafe(no_mangle)]
pub extern "C" fn er_aot_div_f64(a: f64, b: f64) -> f64 {
    a / b
}

/// Unboxed fast math: 64-bit integer addition.
#[unsafe(no_mangle)]
pub extern "C" fn er_aot_add_i64(a: i64, b: i64) -> i64 {
    a.wrapping_add(b)
}

/// Unboxed fast math: 64-bit integer subtraction.
#[unsafe(no_mangle)]
pub extern "C" fn er_aot_sub_i64(a: i64, b: i64) -> i64 {
    a.wrapping_sub(b)
}

/// Unboxed fast math: 64-bit integer multiplication.
#[unsafe(no_mangle)]
pub extern "C" fn er_aot_mul_i64(a: i64, b: i64) -> i64 {
    a.wrapping_mul(b)
}

/// Append a value to a GC array.
#[unsafe(no_mangle)]
pub extern "C" fn er_aot_array_push(arr_ptr: *mut GcObject, val: Value) {
    if !arr_ptr.is_null() {
        unsafe {
            if let GcData::Array(ref mut vec) = (*arr_ptr).data {
                vec.push(val);
            }
        }
    }
}

/// Get length of an array.
#[unsafe(no_mangle)]
pub extern "C" fn er_aot_array_len(arr_ptr: *mut GcObject) -> usize {
    if arr_ptr.is_null() {
        return 0;
    }
    unsafe {
        if let GcData::Array(ref vec) = (*arr_ptr).data {
            vec.len()
        } else {
            0
        }
    }
}

/// Access element at index from a GC array.
#[unsafe(no_mangle)]
pub extern "C" fn er_aot_array_get(arr_ptr: *mut GcObject, index: usize) -> Value {
    if arr_ptr.is_null() {
        return Value::null();
    }
    unsafe {
        if let GcData::Array(ref vec) = (*arr_ptr).data {
            vec.get(index).cloned().unwrap_or(Value::null())
        } else {
            Value::null()
        }
    }
}

/// Store value at index into a GC array.
#[unsafe(no_mangle)]
pub extern "C" fn er_aot_array_set(arr_ptr: *mut GcObject, index: usize, val: Value) {
    if !arr_ptr.is_null() {
        unsafe {
            if let GcData::Array(ref mut vec) = (*arr_ptr).data {
                if index < vec.len() {
                    vec[index] = val;
                }
            }
        }
    }
}
