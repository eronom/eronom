use fnv::FnvHashMap;
use std::borrow::Borrow;
use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use crate::vm::value::Value;
use super::types::{
    BuiltinMethodId, GcBuiltinMethod, GcColor, GcData, GcInlineString, GcObject,
    GcString,
};
use super::gc_allocate;

#[derive(Clone, Copy)]
pub struct InternedStringKey(pub *mut GcObject);

impl InternedStringKey {
    #[inline(always)]
    pub fn as_str(&self) -> &str {
        unsafe {
            match &(*self.0).data {
                GcData::String(s) => s.as_str(),
                _ => "",
            }
        }
    }
}

impl Borrow<str> for InternedStringKey {
    #[inline(always)]
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl PartialEq for InternedStringKey {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0 || self.as_str() == other.as_str()
    }
}

impl Eq for InternedStringKey {}

impl Hash for InternedStringKey {
    #[inline(always)]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

thread_local! {
    pub static STRING_CACHE: RefCell<FnvHashMap<InternedStringKey, *mut GcObject>> = RefCell::new(FnvHashMap::default());
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
        GcString::Heap(Box::from(s))
    };
    gc_allocate(GcData::String(str_data))
}

#[inline(always)]
pub fn gc_alloc_builtin_method(receiver: Value, method: BuiltinMethodId) -> *mut GcObject {
    gc_allocate(GcData::BuiltinMethod(GcBuiltinMethod { receiver, method }))
}

#[inline(always)]
pub fn intern_string(s: &str) -> *mut GcObject {
    STRING_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(&ptr) = cache.get(s) {
            ptr
        } else {
            let ptr = gc_alloc_string(s);
            cache.insert(InternedStringKey(ptr), ptr);
            ptr
        }
    })
}

#[inline(always)]
pub fn get_or_create_string(s: &str) -> *mut GcObject {
    intern_string(s)
}

#[inline(always)]
pub fn json_to_value(json_str: &str) -> Value {
    crate::json::json_parse(json_str).unwrap_or(Value::null())
}
