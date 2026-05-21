use super::bytecode::ArrayMethodType;
use super::gc::{GcObject, GcData};
use std::fmt;

#[derive(Clone, Copy)]
pub enum Value {
    Null,
    Boolean(bool),
    Number(f64),
    String(*mut GcObject),
    Array(*mut GcObject),
    Object(*mut GcObject),
    Function(*mut GcObject),
    NativeFunction(fn(Vec<Value>) -> Value),
    ArrayMethod(*mut GcObject, ArrayMethodType),
}

impl Value {
    #[inline]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(ptr) => unsafe {
                match &(*(*ptr)).data {
                    GcData::String(s) => Some(s.as_str()),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::String(a), Value::String(b)) => unsafe {
                match (&(*(*a)).data, &(*(*b)).data) {
                    (GcData::String(sa), GcData::String(sb)) => sa == sb,
                    _ => false,
                }
            },
            (Value::Array(a), Value::Array(b)) => a == b,
            (Value::Object(a), Value::Object(b)) => a == b,
            (Value::Function(a), Value::Function(b)) => a == b,
            (Value::NativeFunction(a), Value::NativeFunction(b)) => std::ptr::fn_addr_eq(*a, *b),
            (Value::ArrayMethod(a_arr, a_name), Value::ArrayMethod(b_arr, b_name)) => {
                a_arr == b_arr && a_name == b_name
            }
            _ => false,
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "Null"),
            Value::Boolean(b) => write!(f, "Boolean({})", b),
            Value::Number(n) => write!(f, "Number({})", n),
            Value::String(ptr) => unsafe {
                match &(**ptr).data {
                    GcData::String(s) => write!(f, "String({:?})", s),
                    _ => write!(f, "String(invalid gc object)"),
                }
            },
            Value::Array(ptr) => write!(f, "Array({:p})", *ptr),
            Value::Object(ptr) => write!(f, "Object({:p})", *ptr),
            Value::Function(ptr) => write!(f, "Function({:p})", *ptr),
            Value::NativeFunction(func) => write!(f, "NativeFunction({:p})", *func as *const ()),
            Value::ArrayMethod(ptr, method) => write!(f, "ArrayMethod({:p}, {:?})", *ptr, method),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "null"),
            Value::Boolean(b) => write!(f, "{}", b),
            Value::Number(n) => write!(f, "{}", n),
            Value::String(ptr) => unsafe {
                match &(**ptr).data {
                    GcData::String(s) => write!(f, "{}", s),
                    _ => unreachable!(),
                }
            },
            Value::Array(ptr) => unsafe {
                match &(**ptr).data {
                    GcData::Array(arr) => {
                        let items: Vec<String> = arr.iter().map(|v| v.to_string()).collect();
                        write!(f, "[{}]", items.join(", "))
                    }
                    _ => unreachable!(),
                }
            },
            Value::Object(ptr) => unsafe {
                match &(**ptr).data {
                    GcData::Object(obj) => {
                        let items: Vec<String> = obj
                            .iter()
                            .map(|(k, v)| format!("\"{}\": {}", k, v))
                            .collect();
                        write!(f, "{{{}}}", items.join(", "))
                    }
                    _ => unreachable!(),
                }
            },
            Value::Function(_) => write!(f, "[Function]"),
            Value::NativeFunction(_) => write!(f, "[NativeFunction]"),
            Value::ArrayMethod(_, method) => {
                let name = match method {
                    ArrayMethodType::Push => "push",
                    ArrayMethodType::Pop => "pop",
                };
                write!(f, "[ArrayMethod {}]", name)
            }
        }
    }
}
