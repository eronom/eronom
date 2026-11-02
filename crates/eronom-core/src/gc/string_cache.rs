use fnv::FnvHashMap;
use std::cell::RefCell;
use std::rc::Rc;
use crate::vm::value::Value;
use super::types::{
    BuiltinMethodId, GcBuiltinMethod, GcColor, GcData, GcInlineString, GcObject,
    GcString, GcStruct,
};
use super::{gc_allocate, get_pooled_vec};

thread_local! {
    pub static STRING_CACHE: RefCell<FnvHashMap<Rc<str>, *mut GcObject>> = RefCell::new(FnvHashMap::default());
}

pub fn gc_clear_string_cache() {
    STRING_CACHE.with(|cache| cache.borrow_mut().clear());
}

pub fn gc_sweep_string_cache() {
    STRING_CACHE.with(|cache| {
        cache.borrow_mut().retain(|_, ptr| unsafe {
            !ptr.is_null() && (*(*ptr)).color != GcColor::White
        });
    });
}

#[inline(always)]
pub fn gc_alloc_string(s: &str) -> *mut GcObject {
    let len = s.len();
    let str_data = if len <= 31 {
        let mut bytes = [0u8; 31];
        bytes[..len].copy_from_slice(s.as_bytes());
        GcString::Inline(GcInlineString {
            len: len as u8,
            bytes,
        })
    } else {
        GcString::Heap(Rc::from(s))
    };
    gc_allocate(GcData::String(str_data))
}

#[inline(always)]
pub fn gc_alloc_builtin_method(receiver: Value, method: BuiltinMethodId) -> *mut GcObject {
    gc_allocate(GcData::BuiltinMethod(GcBuiltinMethod { receiver, method }))
}

#[inline(always)]
pub fn gc_alloc_string_rc(rc_str: Rc<str>) -> *mut GcObject {
    let len = rc_str.len();
    let str_data = if len <= 31 {
        let mut bytes = [0u8; 31];
        bytes[..len].copy_from_slice(rc_str.as_bytes());
        GcString::Inline(GcInlineString {
            len: len as u8,
            bytes,
        })
    } else {
        GcString::Heap(rc_str)
    };
    gc_allocate(GcData::String(str_data))
}

#[inline(always)]
pub fn intern_string(s: &str) -> *mut GcObject {
    STRING_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(&ptr) = cache.get(s) {
            ptr
        } else {
            let rc_str: Rc<str> = Rc::from(s);
            let ptr = gc_alloc_string(s);
            cache.insert(rc_str, ptr);
            ptr
        }
    })
}

#[inline(always)]
pub fn get_or_create_string(s: &str) -> *mut GcObject {
    intern_string(s)
}

pub fn json_to_value(val: serde_json::Value) -> Value {
    match val {
        serde_json::Value::Null => Value::null(),
        serde_json::Value::Bool(b) => Value::boolean(b),
        serde_json::Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                Value::number(f)
            } else {
                Value::null()
            }
        }
        serde_json::Value::String(s) => {
            let ptr = gc_alloc_string(&s);
            Value::string(ptr)
        }
        serde_json::Value::Array(arr) => {
            let mut elements = get_pooled_vec(arr.len());
            for v in arr {
                elements.push(json_to_value(v));
            }
            let ptr = gc_allocate(GcData::Array(elements));
            Value::array(ptr)
        }
        serde_json::Value::Object(obj) => {
            let mut keys = Vec::with_capacity(obj.len());
            let mut values = Vec::with_capacity(obj.len());
            for (k, v) in obj {
                let key_ptr = intern_string(&k);
                keys.push(Value::string(key_ptr));
                values.push(json_to_value(v));
            }
            let (desc, offsets) = crate::vm::shape::get_or_create_anonymous_shape(&keys);
            let mut fields = get_pooled_vec(keys.len());
            fields.resize(keys.len(), Value::null());
            for i in 0..keys.len() {
                fields[offsets[i]] = values[i];
            }
            let ptr = gc_allocate(GcData::Struct(GcStruct {
                descriptor: desc,
                fields,
            }));
            Value::object(ptr)
        }
    }
}
