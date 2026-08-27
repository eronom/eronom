#![allow(clippy::not_unsafe_ptr_arg_deref)]
use super::value::{
    Value, TAG_STRING, TAG_NATIVE, MapKey
};
use super::bytecode::Function;
use fnv::FnvHashMap;
use std::rc::Rc;
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
pub type ObjectMap = FnvHashMap<MapKey, Value>;

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
    pub field_indices: FnvHashMap<super::value::MapKey, usize>,
    pub methods: FnvHashMap<super::value::MapKey, Value>,
}

impl StructDescriptor {
    pub fn new(
        name: Rc<str>,
        field_indices: FnvHashMap<super::value::MapKey, usize>,
        methods: FnvHashMap<super::value::MapKey, Value>,
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
        let idx = self.descriptor.field_indices.get(&super::value::MapKey(name_val))?;
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
        if let Some(idx) = self.descriptor.field_indices.get(&super::value::MapKey(name_val)) {
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

#[repr(C, u8)]
pub enum GcData {
    Empty,
    String(Rc<str>),
    Array(Vec<Value>),
    Object(ObjectMap),
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

pub struct GcState {
    pub head: *mut GcObject,
    pub alloc_count: usize,
    pub alloc_threshold: usize,
    pub live_count: usize,
    pub phase: GcPhase,
    pub gray_stack: Vec<*mut GcObject>,
    pub sweep_ptr: *mut GcObject,
    pub prev_sweep_ptr: *mut GcObject,
    pub free_list: Vec<*mut GcObject>,
    pub current_chunk_ptr: *mut GcObject,
    pub chunk_remaining: usize,
    pub chunks: Vec<*mut GcObject>,
    pub vector_pool: Vec<Vec<Value>>,
    pub map_pool: Vec<ObjectMap>,
}

thread_local! {
    pub static GC_STATE: std::cell::UnsafeCell<GcState> = const { std::cell::UnsafeCell::new(GcState {
        head: std::ptr::null_mut(),
        alloc_count: 0,
        alloc_threshold: 10000,
        live_count: 0,
        phase: GcPhase::Pause,
        gray_stack: Vec::new(),
        sweep_ptr: std::ptr::null_mut(),
        prev_sweep_ptr: std::ptr::null_mut(),
        free_list: Vec::new(),
        current_chunk_ptr: std::ptr::null_mut(),
        chunk_remaining: 0,
        chunks: Vec::new(),
        vector_pool: Vec::new(),
        map_pool: Vec::new(),
    }) };
    pub static GC_ROOTS: RefCell<Vec<Box<dyn Fn()>>> = RefCell::new(Vec::new());
}

pub static GC_NEEDS_STEP: AtomicBool = AtomicBool::new(false);

#[inline(always)]
pub fn gc_with_state<R>(f: impl FnOnce(&mut GcState) -> R) -> R {
    GC_STATE.with(|state| unsafe { f(&mut *state.get()) })
}

#[inline(always)]
pub fn get_pooled_vec(capacity: usize) -> Vec<Value> {
    GC_STATE.with(|state| unsafe {
        let s = &mut *state.get();
        if let Some(mut v) = s.vector_pool.pop() {
            if v.capacity() < capacity {
                v.reserve(capacity - v.capacity());
            }
            v
        } else {
            let cap = capacity.max(8);
            for _ in 0..63 {
                s.vector_pool.push(Vec::with_capacity(cap));
            }
            Vec::with_capacity(cap)
        }
    })
}

#[inline(always)]
pub fn get_pooled_map(capacity: usize) -> ObjectMap {
    GC_STATE.with(|state| unsafe {
        let s = &mut *state.get();
        if let Some(mut m) = s.map_pool.pop() {
            if m.capacity() < capacity {
                m.reserve(capacity - m.capacity());
            }
            m
        } else {
            let cap = capacity.max(4);
            for _ in 0..31 {
                s.map_pool.push(ObjectMap::with_capacity_and_hasher(cap, Default::default()));
            }
            ObjectMap::with_capacity_and_hasher(cap, Default::default())
        }
    })
}

#[inline(always)]
pub fn gc_alloc_array(slice: &[Value]) -> *mut GcObject {
    GC_STATE.with(|state| unsafe {
        let s = &mut *state.get();
        let count = slice.len();
        let cap = if count < 4 { 8 } else { count + (count >> 1) };
        let mut elements = if let Some(mut v) = s.vector_pool.pop() {
            if v.capacity() < cap {
                v.reserve(cap - v.capacity());
            }
            v.clear();
            v
        } else {
            for _ in 0..63 {
                s.vector_pool.push(Vec::with_capacity(cap));
            }
            Vec::with_capacity(cap)
        };
        elements.extend_from_slice(slice);
        let ptr = gc_alloc_object(s, GcData::Array(elements));
        (*ptr).next = s.head;
        s.head = ptr;
        s.alloc_count += 1;
        if s.alloc_count >= s.alloc_threshold {
            GC_NEEDS_STEP.store(true, Ordering::Relaxed);
        }
        ptr
    })
}

const GC_POOL_MAX: usize = 2048;

#[inline(always)]
pub fn gc_recycle_data(state: &mut GcState, data: &mut GcData) {
    unsafe {
        match data {
            GcData::Array(arr) => {
                let mut vec = std::ptr::read(arr);
                vec.clear();
                if state.vector_pool.len() < GC_POOL_MAX {
                    state.vector_pool.push(vec);
                }
            }
            GcData::Object(obj) => {
                let mut map = std::ptr::read(obj);
                map.clear();
                if state.map_pool.len() < GC_POOL_MAX {
                    state.map_pool.push(map);
                }
            }
            GcData::Struct(s) => {
                let mut s_val = std::ptr::read(s);
                s_val.fields.clear();
                if state.vector_pool.len() < GC_POOL_MAX {
                    state.vector_pool.push(s_val.fields);
                }
            }
            _ => {
                std::ptr::drop_in_place(data);
            }
        }
        std::ptr::write(data, GcData::Empty);
    }
}

const CHUNK_COUNT: usize = 4096;

#[inline(always)]
pub fn gc_alloc_object(state: &mut GcState, data: GcData) -> *mut GcObject {
    if let Some(ptr) = state.free_list.pop() {
        unsafe {
            std::ptr::write(ptr, GcObject {
                color: GcColor::White,
                next: std::ptr::null_mut(),
                data,
            });
        }
        ptr
    } else if state.chunk_remaining > 0 {
        let ptr = state.current_chunk_ptr;
        unsafe {
            state.current_chunk_ptr = state.current_chunk_ptr.add(1);
            state.chunk_remaining -= 1;
            std::ptr::write(ptr, GcObject {
                color: GcColor::White,
                next: std::ptr::null_mut(),
                data,
            });
        }
        ptr
    } else {
        let layout = std::alloc::Layout::array::<GcObject>(CHUNK_COUNT).unwrap();
        let raw = unsafe { std::alloc::alloc(layout) as *mut GcObject };
        if raw.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        state.chunks.push(raw);
        state.current_chunk_ptr = unsafe { raw.add(1) };
        state.chunk_remaining = CHUNK_COUNT - 1;
        unsafe {
            std::ptr::write(raw, GcObject {
                color: GcColor::White,
                next: std::ptr::null_mut(),
                data,
            });
            raw
        }
    }
}

#[inline(always)]
pub fn gc_dealloc_object(state: &mut GcState, ptr: *mut GcObject) {
    unsafe {
        gc_recycle_data(state, &mut (*ptr).data);
    }
    state.free_list.push(ptr);
}

#[inline(always)]
pub fn gc_allocate(data: GcData) -> *mut GcObject {
    GC_STATE.with(|state| unsafe {
        let s_ref = &mut *state.get();
        let ptr = gc_alloc_object(s_ref, data);
        (*ptr).next = s_ref.head;
        s_ref.head = ptr;

        if let GcPhase::Sweep = s_ref.phase {
            if s_ref.prev_sweep_ptr.is_null() {
                s_ref.prev_sweep_ptr = ptr;
            }
        }

        s_ref.alloc_count += 1;
        if s_ref.alloc_count >= s_ref.alloc_threshold {
            GC_NEEDS_STEP.store(true, Ordering::Relaxed);
        }
        ptr
    })
}

#[inline(always)]
pub fn gc_free_all() {
    unsafe {
        GC_STATE.with(|state| {
            let s_ref = &mut *state.get();
            let mut curr = s_ref.head;
            s_ref.head = std::ptr::null_mut();
            while !curr.is_null() {
                let next = (*curr).next;
                gc_recycle_data(s_ref, &mut (*curr).data);
                curr = next;
            }
            s_ref.free_list.clear();
            s_ref.current_chunk_ptr = std::ptr::null_mut();
            s_ref.chunk_remaining = 0;
            let layout = std::alloc::Layout::array::<GcObject>(CHUNK_COUNT).unwrap();
            for chunk in s_ref.chunks.drain(..) {
                std::alloc::dealloc(chunk as *mut u8, layout);
            }
            s_ref.vector_pool.clear();
            s_ref.map_pool.clear();
            s_ref.alloc_count = 0;
            s_ref.live_count = 0;
            s_ref.phase = GcPhase::Pause;
            s_ref.gray_stack.clear();
            s_ref.sweep_ptr = std::ptr::null_mut();
            s_ref.prev_sweep_ptr = std::ptr::null_mut();
            GC_NEEDS_STEP.store(false, Ordering::Relaxed);
        });
        gc_clear_string_cache();
        super::shape::reset_shape_state();
        crate::jit::helpers::reset_global_ic();
    }
}


#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn gc_mark_value(val: &Value) {
    if !val.is_number() && !val.is_inline_string() {
        let tag = val.0 & 0xffff_0000_0000_0000;
        if tag >= TAG_STRING && tag != TAG_NATIVE {
            let ptr = val.as_gc_ptr();
            unsafe {
                if !ptr.is_null() && (*ptr).color == GcColor::White {
                    (*ptr).color = GcColor::Gray;
                    GC_STATE.with(|state| (*state.get()).gray_stack.push(ptr));
                }
            }
        }
    }
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn gc_mark_object(ptr: *mut GcObject) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        if (*ptr).color == GcColor::White {
            (*ptr).color = GcColor::Gray;
            GC_STATE.with(|state| (*state.get()).gray_stack.push(ptr));
        }
    }
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn gc_blacken_object(ptr: *mut GcObject) {
    unsafe {
        if ptr.is_null() {
            return;
        }
        (*ptr).color = GcColor::Black;
        match &(*ptr).data {
            GcData::Empty => {}
            GcData::String(_) => {}
            GcData::Array(arr) => {
                for val in arr {
                    gc_mark_value(val);
                }
            }
            GcData::Object(obj) => {
                for (key, val) in obj {
                    gc_mark_value(&key.0);
                    gc_mark_value(val);
                }
            }
            GcData::Struct(s) => {
                for val in &s.fields {
                    gc_mark_value(val);
                }
            }
            GcData::Function(func) => {
                for constant in &func.chunk.constants {
                    gc_mark_value(constant);
                }
            }
            GcData::Promise(prom) => {
                if let Ok(stack) = prom.suspended_stack.lock() {
                    for val in stack.iter() {
                        gc_mark_value(val);
                    }
                }
                if let Ok(frames) = prom.suspended_frames.lock() {
                    for frame in frames.iter() {
                        let fn_val = Value::function(frame.function);
                        gc_mark_value(&fn_val);
                    }
                }
            }
            GcData::BoundMethod(bm) => {
                gc_mark_value(&bm.receiver);
                gc_mark_object(bm.function);
            }
            GcData::BuiltinMethod(bm) => {
                gc_mark_value(&bm.receiver);
            }
            GcData::StructConstructor(_) => {}
            GcData::Closure(c) => {
                gc_mark_object(c.function);
                for &upval_ptr in &c.upvalues {
                    gc_mark_object(upval_ptr);
                }
            }
            GcData::Upvalue(u) => {
                if let UpvalueLocation::Closed(ref val) = u.location {
                    gc_mark_value(val);
                }
            }
        }
    }
}

pub fn mark_value(val: &Value) {
    gc_mark_value(val);
}

pub fn mark_object(ptr: *mut GcObject) {
    gc_mark_object(ptr);
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn gc_write_barrier(parent: *mut GcObject, child: &Value) {
    unsafe {
        if parent.is_null() {
            return;
        }
        if (*parent).color == GcColor::Black && !child.is_number() && !child.is_inline_string() {
            let tag = child.0 & 0xffff_0000_0000_0000;
            if tag >= TAG_STRING && tag != TAG_NATIVE {
                let child_ptr = child.as_gc_ptr();
                if !child_ptr.is_null() && (*child_ptr).color == GcColor::White {
                    (*child_ptr).color = GcColor::Gray;
                    GC_STATE.with(|state| {
                        (*state.get()).gray_stack.push(child_ptr);
                    });
                }
            }
        }
    }
}

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
    let rc_str: Rc<str> = Rc::from(s);
    gc_allocate(GcData::String(rc_str))
}

#[inline(always)]
pub fn gc_alloc_builtin_method(receiver: Value, method: BuiltinMethodId) -> *mut GcObject {
    gc_allocate(GcData::BuiltinMethod(GcBuiltinMethod { receiver, method }))
}

#[inline(always)]
pub fn gc_alloc_string_rc(rc_str: Rc<str>) -> *mut GcObject {
    gc_allocate(GcData::String(rc_str))
}

#[inline(always)]
pub fn intern_string(s: &str) -> *mut GcObject {
    STRING_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(&ptr) = cache.get(s) {
            ptr
        } else {
            let rc_str: Rc<str> = Rc::from(s);
            let ptr = gc_allocate(GcData::String(rc_str.clone()));
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
