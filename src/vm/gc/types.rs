use fnv::FnvHashMap;
use std::rc::Rc;
use crate::vm::value::{MapKey, Value};
use crate::vm::bytecode::Function;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GcColor {
    White,
    Gray,
    Black,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GcPhase {
    Pause,
    Mark,
    Atomic,
    Sweep,
}

#[derive(Clone)]
pub enum PromiseState {
    Pending,
    Fulfilled(Value),
    Rejected(String),
}

#[derive(Clone)]
pub struct GcPromise {
    pub state: std::sync::Arc<std::sync::Mutex<PromiseState>>,
    pub suspended_stack: std::sync::Arc<std::sync::Mutex<Vec<Value>>>,
    pub suspended_frames: std::sync::Arc<std::sync::Mutex<Vec<crate::vm::execute::CallFrame>>>,
}

static NEXT_DESCRIPTOR_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

#[repr(C)]
#[derive(Clone, Debug)]
pub struct StructDescriptor {
    pub id: u32,
    pub _padding: u32,
    pub fast_field_count: usize,
    pub fast_fields: [(Value, usize); 8],
    pub name: Rc<str>,
    pub field_indices: FnvHashMap<MapKey, usize>,
    pub methods: FnvHashMap<MapKey, Value>,
}

impl StructDescriptor {
    pub fn new(
        name: Rc<str>,
        field_indices: FnvHashMap<MapKey, usize>,
        methods: FnvHashMap<MapKey, Value>,
    ) -> Self {
        let mut fast_fields = [(Value::null(), 0); 8];
        let mut count = 0;
        for (&map_key, &idx) in &field_indices {
            if count < 8 {
                fast_fields[count] = (map_key.0, idx);
                count += 1;
            }
        }
        let id = NEXT_DESCRIPTOR_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self {
            id,
            _padding: 0,
            fast_field_count: count,
            fast_fields,
            name,
            field_indices,
            methods,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuiltinMethodId {
    // String methods
    StringToUpperCase,
    StringToLowerCase,
    StringTrim,
    StringTrimStart,
    StringTrimEnd,
    StringSplit,
    StringSlice,
    StringSubstring,
    StringIndexOf,
    StringLastIndexOf,
    StringIncludes,
    StringStartsWith,
    StringEndsWith,
    StringReplace,
    StringReplaceAll,
    StringCharAt,
    StringCharCodeAt,
    StringRepeat,
    StringPadStart,
    StringPadEnd,
    StringConcat,

    // Array methods
    ArrayPush,
    ArrayPop,
    ArrayShift,
    ArrayUnshift,
    ArrayMap,
    ArrayFilter,
    ArrayReduce,
    ArrayForEach,
    ArrayFind,
    ArrayFindIndex,
    ArraySome,
    ArrayEvery,
    ArrayIncludes,
    ArrayIndexOf,
    ArrayLastIndexOf,
    ArraySlice,
    ArrayJoin,
    ArrayConcat,
    ArrayReverse,
    ArraySort,
    ArrayFlat,
    ArrayFlatMap,
    ArrayFill,

    // Object methods
    ObjectKeys,
    ObjectValues,
    ObjectEntries,
    ObjectHasOwnProperty,
}

#[derive(Clone)]
pub struct GcBuiltinMethod {
    pub receiver: Value,
    pub method: BuiltinMethodId,
}

#[derive(Clone)]
pub struct GcBoundMethod {
    pub receiver: Value,
    pub function: *mut GcObject,
}

#[repr(C)]
#[derive(Clone)]
pub struct GcStruct {
    pub descriptor: Rc<StructDescriptor>,
    pub fields: Vec<Value>,
}

impl GcStruct {
    #[inline(always)]
    pub fn get_field(&self, name_val: Value) -> Option<Value> {
        for i in 0..self.descriptor.fast_field_count {
            let (k, idx) = self.descriptor.fast_fields[i];
            if k.0 == name_val.0 {
                return Some(self.fields[idx]);
            }
        }
        let idx = self.descriptor.field_indices.get(&MapKey(name_val))?;
        Some(self.fields[*idx])
    }

    pub fn get_field_by_name(&self, name: &str) -> Option<Value> {
        for (map_key, &idx) in &self.descriptor.field_indices {
            let k_str = map_key.0.as_str().unwrap_or("");
            if k_str == name {
                return Some(self.fields[idx]);
            }
        }
        None
    }

    #[inline(always)]
    pub fn set_field(&mut self, name_val: Value, val: Value) -> bool {
        for i in 0..self.descriptor.fast_field_count {
            let (k, idx) = self.descriptor.fast_fields[i];
            if k.0 == name_val.0 {
                self.fields[idx] = val;
                return true;
            }
        }
        if let Some(idx) = self.descriptor.field_indices.get(&MapKey(name_val)) {
            self.fields[*idx] = val;
            true
        } else {
            false
        }
    }
}

#[derive(Clone, Debug)]
pub enum UpvalueLocation {
    Open(usize),
    Closed(Value),
}

#[derive(Clone, Debug)]
pub struct GcUpvalue {
    pub location: UpvalueLocation,
}

#[derive(Clone)]
pub struct GcClosure {
    pub function: *mut GcObject,
    pub upvalues: Vec<*mut GcObject>,
}

#[repr(C)]
#[derive(Clone)]
pub struct GcInlineString {
    pub len: u8,
    pub bytes: [u8; 31],
}

#[derive(Clone)]
pub enum GcString {
    Inline(GcInlineString),
    Heap(Rc<str>),
}

impl GcString {
    #[inline(always)]
    pub fn as_str(&self) -> &str {
        match self {
            GcString::Inline(inline) => unsafe {
                std::str::from_utf8_unchecked(&inline.bytes[..inline.len as usize])
            },
            GcString::Heap(s) => s.as_ref(),
        }
    }
}

#[repr(C, u8)]
pub enum GcData {
    Empty,
    String(GcString),
    Array(Vec<Value>),
    Object(FnvHashMap<MapKey, Value>),
    Function(Box<Function>),
    Promise(GcPromise),
    Struct(GcStruct),
    BoundMethod(GcBoundMethod),
    BuiltinMethod(GcBuiltinMethod),
    StructConstructor(Rc<StructDescriptor>),
    Closure(GcClosure),
    Upvalue(GcUpvalue),
}

#[repr(C)]
pub struct GcObject {
    pub color: GcColor,
    pub next: *mut GcObject,
    pub data: GcData,
}
