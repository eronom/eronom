use super::gc::{GcObject, GcData};
use std::fmt;

pub const TAG_NUMBER_MASK: u64 = 0xfff0_0000_0000_0000;
pub const TAG_NULL: u64        = 0xfff1_0000_0000_0000;
pub const TAG_FALSE: u64       = 0xfff2_0000_0000_0000;
pub const TAG_TRUE: u64        = 0xfff3_0000_0000_0000;
pub const TAG_STRING: u64      = 0xfff4_0000_0000_0000;
pub const TAG_ARRAY: u64       = 0xfff5_0000_0000_0000;
pub const TAG_OBJECT: u64      = 0xfff6_0000_0000_0000;
pub const TAG_FUNCTION: u64    = 0xfff7_0000_0000_0000;
pub const TAG_NATIVE: u64      = 0xfff8_0000_0000_0000;
pub const TAG_METHOD_PUSH: u64 = 0xfff9_0000_0000_0000;
pub const TAG_METHOD_POP: u64  = 0xfffa_0000_0000_0000;
pub const TAG_METHOD_JSON: u64 = 0xfffb_0000_0000_0000;
pub const TAG_METHOD_TEXT: u64 = 0xfffc_0000_0000_0000;
pub const TAG_METHOD_SEND_JSON: u64 = 0xfffd_0000_0000_0000;
pub const TAG_PROMISE: u64     = 0xfffe_0000_0000_0000;
pub const TAG_METHOD_RESOLVE: u64 = 0xffff_0000_0000_0000;
pub const PTR_MASK: u64        = 0x0000_ffff_ffff_ffff;

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct Value(pub u64);

impl Value {
    #[inline(always)]
    pub fn null() -> Self {
        Value(TAG_NULL)
    }

    #[inline(always)]
    pub fn boolean(b: bool) -> Self {
        Value(TAG_FALSE + (b as u64 * 0x0001_0000_0000_0000))
    }

    #[inline(always)]
    pub fn number(n: f64) -> Self {
        let bits = n.to_bits();
        if (bits & TAG_NUMBER_MASK) == TAG_NUMBER_MASK {
            // NaN or infinity that overlaps with our tags - box it as a normal number representation
            Value(0x7ff8_0000_0000_0000)
        } else {
            Value(bits)
        }
    }

    #[inline(always)]
    pub fn number_unchecked(n: f64) -> Self {
        Value(n.to_bits())
    }

    #[inline(always)]
    pub fn string(ptr: *mut GcObject) -> Self {
        Value(TAG_STRING | (ptr as u64 & PTR_MASK))
    }

    #[inline(always)]
    pub fn array(ptr: *mut GcObject) -> Self {
        Value(TAG_ARRAY | (ptr as u64 & PTR_MASK))
    }

    #[inline(always)]
    pub fn object(ptr: *mut GcObject) -> Self {
        Value(TAG_OBJECT | (ptr as u64 & PTR_MASK))
    }

    #[inline(always)]
    pub fn function(ptr: *mut GcObject) -> Self {
        Value(TAG_FUNCTION | (ptr as u64 & PTR_MASK))
    }

    #[inline(always)]
    pub fn promise(ptr: *mut GcObject) -> Self {
        Value(TAG_PROMISE | (ptr as u64 & PTR_MASK))
    }

    #[inline(always)]
    pub fn native_function(f: fn(Vec<Value>) -> Value) -> Self {
        Value(TAG_NATIVE | (f as *const () as u64 & PTR_MASK))
    }

    #[inline(always)]
    pub fn array_method_push(ptr: *mut GcObject) -> Self {
        Value(TAG_METHOD_PUSH | (ptr as u64 & PTR_MASK))
    }

    #[inline(always)]
    pub fn array_method_pop(ptr: *mut GcObject) -> Self {
        Value(TAG_METHOD_POP | (ptr as u64 & PTR_MASK))
    }

    #[inline(always)]
    pub fn is_number(self) -> bool {
        self.0 < TAG_NUMBER_MASK
    }

    #[inline(always)]
    pub fn as_number(self) -> f64 {
        f64::from_bits(self.0)
    }

    #[inline(always)]
    pub fn is_null(self) -> bool {
        self.0 == TAG_NULL
    }

    #[inline(always)]
    pub fn is_boolean(self) -> bool {
        (self.0 & 0xffff_0000_0000_0000) == TAG_FALSE || (self.0 & 0xffff_0000_0000_0000) == TAG_TRUE
    }

    #[inline(always)]
    pub fn as_boolean(self) -> bool {
        self.0 == TAG_TRUE
    }

    #[inline(always)]
    pub fn is_string(self) -> bool {
        (self.0 & 0xffff_0000_0000_0000) == TAG_STRING
    }

    #[inline(always)]
    pub fn is_array(self) -> bool {
        (self.0 & 0xffff_0000_0000_0000) == TAG_ARRAY
    }

    #[inline(always)]
    pub fn is_object(self) -> bool {
        (self.0 & 0xffff_0000_0000_0000) == TAG_OBJECT
    }

    #[inline(always)]
    pub fn is_function(self) -> bool {
        (self.0 & 0xffff_0000_0000_0000) == TAG_FUNCTION
    }

    #[inline(always)]
    pub fn is_native_function(self) -> bool {
        (self.0 & 0xffff_0000_0000_0000) == TAG_NATIVE
    }

    #[inline(always)]
    pub fn is_array_method_push(self) -> bool {
        (self.0 & 0xffff_0000_0000_0000) == TAG_METHOD_PUSH
    }

    #[inline(always)]
    pub fn is_array_method_pop(self) -> bool {
        (self.0 & 0xffff_0000_0000_0000) == TAG_METHOD_POP
    }

    #[inline(always)]
    pub fn is_method_json(self) -> bool {
        (self.0 & 0xffff_0000_0000_0000) == TAG_METHOD_JSON
    }

    #[inline(always)]
    pub fn is_method_text(self) -> bool {
        (self.0 & 0xffff_0000_0000_0000) == TAG_METHOD_TEXT
    }

    #[inline(always)]
    pub fn is_method_send_json(self) -> bool {
        (self.0 & 0xffff_0000_0000_0000) == TAG_METHOD_SEND_JSON
    }

    #[inline(always)]
    pub fn is_method_resolve(self) -> bool {
        (self.0 & 0xffff_0000_0000_0000) == TAG_METHOD_RESOLVE
    }

    #[inline(always)]
    pub fn is_promise(self) -> bool {
        (self.0 & 0xffff_0000_0000_0000) == TAG_PROMISE
    }

    #[inline(always)]
    pub fn as_gc_ptr(self) -> *mut GcObject {
        (self.0 & PTR_MASK) as *mut GcObject
    }

    #[inline(always)]
    pub fn as_native_fn(self) -> fn(Vec<Value>) -> Value {
        unsafe { std::mem::transmute(self.0 & PTR_MASK) }
    }

    #[inline]
    pub fn as_str(&self) -> Option<&str> {
        if self.is_string() {
            unsafe {
                match &(*self.as_gc_ptr()).data {
                    GcData::String(s) => Some(s),
                    _ => None,
                }
            }
        } else {
            None
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        if self.is_number() && other.is_number() {
            self.as_number() == other.as_number()
        } else if self.is_string() && other.is_string() {
            unsafe {
                let a = self.as_gc_ptr();
                let b = other.as_gc_ptr();
                if a == b {
                    return true;
                }
                match (&(*a).data, &(*b).data) {
                    (GcData::String(sa), GcData::String(sb)) => sa == sb,
                    _ => false,
                }
            }
        } else {
            self.0 == other.0
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_null() {
            write!(f, "Null")
        } else if self.is_boolean() {
            write!(f, "Boolean({})", self.as_boolean())
        } else if self.is_number() {
            write!(f, "Number({})", self.as_number())
        } else if self.is_string() {
            unsafe {
                match &(*self.as_gc_ptr()).data {
                    GcData::String(s) => write!(f, "String({:?})", s),
                    _ => write!(f, "String(invalid gc object)"),
                }
            }
        } else if self.is_array() {
            write!(f, "Array({:p})", self.as_gc_ptr())
        } else if self.is_object() {
            write!(f, "Object({:p})", self.as_gc_ptr())
        } else if self.is_function() {
            write!(f, "Function({:p})", self.as_gc_ptr())
        } else if self.is_native_function() {
            write!(f, "NativeFunction({:p})", self.as_native_fn() as *const ())
        } else if self.is_array_method_push() {
            write!(f, "ArrayMethod({:p}, Push)", self.as_gc_ptr())
        } else if self.is_array_method_pop() {
            write!(f, "ArrayMethod({:p}, Pop)", self.as_gc_ptr())
        } else if self.is_method_json() {
            write!(f, "MethodJson({:p})", self.as_gc_ptr())
        } else if self.is_method_text() {
            write!(f, "MethodText({:p})", self.as_gc_ptr())
        } else if self.is_method_send_json() {
            write!(f, "MethodSendJson({:p})", (self.0 & PTR_MASK) as *const ())
        } else if self.is_method_resolve() {
            write!(f, "MethodResolve({:p})", (self.0 & PTR_MASK) as *const ())
        } else if self.is_promise() {
            write!(f, "Promise({:p})", self.as_gc_ptr())
        } else {
            write!(f, "Value(invalid 0x{:x})", self.0)
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_null() {
            write!(f, "null")
        } else if self.is_boolean() {
            write!(f, "{}", self.as_boolean())
        } else if self.is_number() {
            write!(f, "{}", self.as_number())
        } else if self.is_string() {
            unsafe {
                match &(*self.as_gc_ptr()).data {
                    GcData::String(s) => write!(f, "{}", s),
                    _ => unreachable!(),
                }
            }
        } else if self.is_array() {
            unsafe {
                match &(*self.as_gc_ptr()).data {
                    GcData::Array(arr) => {
                        let items: Vec<String> = arr.iter().map(|v| v.to_string()).collect();
                        write!(f, "[{}]", items.join(", "))
                    }
                    _ => unreachable!(),
                }
            }
        } else if self.is_object() {
            unsafe {
                match &(*self.as_gc_ptr()).data {
                    GcData::Object(obj) => {
                        let items: Vec<String> = obj
                            .iter()
                            .map(|(k, v)| {
                                let s = match &(*k.0.as_gc_ptr()).data {
                                    GcData::String(s) => s.as_ref(),
                                    _ => unreachable!(),
                                };
                                format!("\"{}\": {}", s, v)
                            })
                            .collect();
                        write!(f, "{{{}}}", items.join(", "))
                    }
                    GcData::Struct(s) => {
                        let mut items = vec![String::new(); s.fields.len()];
                        for (map_key, &idx) in &s.descriptor.field_indices {
                            let name = map_key.0.as_str().unwrap_or("");
                            items[idx] = format!("\"{}\": {}", name, s.fields[idx]);
                        }
                        write!(f, "{{{}}}", items.join(", "))
                    }
                    GcData::StructConstructor(desc) => {
                        write!(f, "[Constructor {}]", desc.name)
                    }
                    _ => unreachable!(),
                }
            }
        } else if self.is_function() {
            write!(f, "[Function]")
        } else if self.is_native_function() {
            write!(f, "[NativeFunction]")
        } else if self.is_array_method_push() {
            write!(f, "[ArrayMethod push]")
        } else if self.is_array_method_pop() {
            write!(f, "[ArrayMethod pop]")
        } else if self.is_method_json() {
            write!(f, "[Method json]")
        } else if self.is_method_text() {
            write!(f, "[Method text]")
        } else if self.is_method_send_json() {
            write!(f, "[Method sendJson]")
        } else if self.is_method_resolve() {
            write!(f, "[Method resolve]")
        } else if self.is_promise() {
            write!(f, "[Promise]")
        } else {
            write!(f, "[Unknown]")
        }
    }
}

#[inline(always)]
pub fn push_positive_integer(s: &mut String, mut n: u64) {
    if n == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut idx = 20;
    while n > 0 {
        idx -= 1;
        buf[idx] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    s.push_str(unsafe { std::str::from_utf8_unchecked(&buf[idx..]) });
}

use std::cell::RefCell;
thread_local! {
    pub static ADD_SCRATCH: RefCell<String> = RefCell::new(String::with_capacity(128));
}

#[derive(Clone, Copy, Debug)]
pub struct MapKey(pub Value);

impl PartialEq for MapKey {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        if self.0.0 == other.0.0 {
            return true;
        }
        if self.0.is_string() && other.0.is_string() {
            unsafe {
                let s1 = match &(*self.0.as_gc_ptr()).data {
                    GcData::String(s) => s.as_ref(),
                    _ => unreachable!(),
                };
                let s2 = match &(*other.0.as_gc_ptr()).data {
                    GcData::String(s) => s.as_ref(),
                    _ => unreachable!(),
                };
                s1 == s2
            }
        } else {
            false
        }
    }
}

impl Eq for MapKey {}

impl std::hash::Hash for MapKey {
    #[inline(always)]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        if self.0.is_string() {
            unsafe {
                match &(*self.0.as_gc_ptr()).data {
                    GcData::String(s) => s.as_ref().hash(state),
                    _ => unreachable!(),
                }
            }
        } else {
            self.0.0.hash(state);
        }
    }
}
