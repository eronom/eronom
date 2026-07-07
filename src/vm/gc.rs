#![allow(clippy::not_unsafe_ptr_arg_deref)]
use super::value::{
    Value, TAG_NUMBER_MASK, TAG_STRING, TAG_FUNCTION, TAG_METHOD_PUSH, TAG_METHOD_POP, MapKey
};
use super::bytecode::Function;
use fnv::FnvHashMap;
use std::rc::Rc;
use std::cell::RefCell;
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

#[derive(Clone, Debug)]
pub struct StructDescriptor {
    pub name: Rc<str>,
    pub field_indices: FnvHashMap<super::value::MapKey, usize>,
    pub methods: FnvHashMap<super::value::MapKey, Value>,
}

#[derive(Clone)]
pub struct GcBoundMethod {
    pub receiver: Value,
    pub function: *mut GcObject,
}

#[derive(Clone)]
pub struct GcStruct {
    pub descriptor: Rc<StructDescriptor>,
    pub fields: Vec<Value>,
}

impl GcStruct {
    pub fn get_field(&self, name_val: Value) -> Option<Value> {
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

    pub fn set_field(&mut self, name_val: Value, val: Value) -> bool {
        if let Some(idx) = self.descriptor.field_indices.get(&super::value::MapKey(name_val)) {
            self.fields[*idx] = val;
            true
        } else {
            false
        }
    }
}

pub enum GcData {
    Empty,
    String(Rc<str>),
    Array(Vec<Value>),
    Object(ObjectMap),
    Function(Function),
    Promise(GcPromise),
    Struct(GcStruct),
    BoundMethod(GcBoundMethod),
    StructConstructor(Rc<StructDescriptor>),
}

pub struct GcObject {
    pub color: GcColor,
    pub next: *mut GcObject,
    pub data: GcData,
}

pub struct GcState {
    pub head: *mut GcObject,
    pub alloc_count: usize,
    pub phase: GcPhase,
    pub gray_stack: Vec<*mut GcObject>,
    pub sweep_ptr: *mut GcObject,
    pub prev_sweep_ptr: *mut GcObject,
    pub free_list: Vec<*mut GcObject>,
    pub vector_pool: Vec<Vec<Value>>,
    pub map_pool: Vec<ObjectMap>,
}

thread_local! {
    pub static GC_STATE: RefCell<GcState> = RefCell::new(GcState {
        head: std::ptr::null_mut(),
        alloc_count: 0,
        phase: GcPhase::Pause,
        gray_stack: Vec::new(),
        sweep_ptr: std::ptr::null_mut(),
        prev_sweep_ptr: std::ptr::null_mut(),
        free_list: Vec::new(),
        vector_pool: Vec::new(),
        map_pool: Vec::new(),
    });
    pub static GC_ROOTS: RefCell<Vec<Box<dyn Fn()>>> = RefCell::new(Vec::new());
}

pub static mut GC_NEEDS_STEP: bool = false;

#[inline(always)]
pub fn get_pooled_vec(capacity: usize) -> Vec<Value> {
    GC_STATE.with(|state| {
        let mut s = state.borrow_mut();
        if let Some(mut vec) = s.vector_pool.pop() {
            if vec.capacity() < capacity {
                vec.reserve(capacity - vec.capacity());
            }
            vec
        } else {
            Vec::with_capacity(capacity)
        }
    })
}

#[inline(always)]
pub fn get_pooled_map(capacity: usize) -> ObjectMap {
    GC_STATE.with(|state| {
        let mut s = state.borrow_mut();
        if let Some(mut map) = s.map_pool.pop() {
            map.reserve(capacity);
            map
        } else {
            ObjectMap::with_capacity_and_hasher(capacity, Default::default())
        }
    })
}

#[inline(always)]
pub fn gc_recycle_data(state: &mut GcState, data: &mut GcData) {
    unsafe {
        match data {
            GcData::Array(arr) => {
                let mut vec = std::ptr::read(arr);
                vec.clear();
                state.vector_pool.push(vec);
            }
            GcData::Object(obj) => {
                let mut map = std::ptr::read(obj);
                map.clear();
                state.map_pool.push(map);
            }
            GcData::Struct(s) => {
                let mut s_val = std::ptr::read(s);
                s_val.fields.clear();
                state.vector_pool.push(s_val.fields);
            }
            _ => {
                std::ptr::drop_in_place(data);
            }
        }
        std::ptr::write(data, GcData::Empty);
    }
}

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
    } else {
        let obj = Box::new(GcObject {
            color: GcColor::White,
            next: std::ptr::null_mut(),
            data,
        });
        Box::into_raw(obj)
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
    GC_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let s_ref = &mut *state;
        let ptr = gc_alloc_object(s_ref, data);
        unsafe {
            (*ptr).next = s_ref.head;
        }
        s_ref.head = ptr;

        if let GcPhase::Sweep = s_ref.phase {
            if s_ref.prev_sweep_ptr.is_null() {
                s_ref.prev_sweep_ptr = ptr;
            }
        }

        s_ref.alloc_count += 1;
        if s_ref.alloc_count >= 10000 {
            unsafe { GC_NEEDS_STEP = true; }
        }
        ptr
    })
}

#[inline(always)]
pub fn gc_free_all() {
    unsafe {
        GC_STATE.with(|state| {
            let mut state = state.borrow_mut();
            let s_ref = &mut *state;
            let mut curr = s_ref.head;
            s_ref.head = std::ptr::null_mut();
            while !curr.is_null() {
                let next = (*curr).next;
                gc_recycle_data(s_ref, &mut (*curr).data);
                let _ = Box::from_raw(curr);
                curr = next;
            }
            let free_list = std::mem::take(&mut s_ref.free_list);
            for ptr in free_list {
                let _ = Box::from_raw(ptr);
            }
            s_ref.vector_pool.clear();
            s_ref.map_pool.clear();
            s_ref.alloc_count = 0;
            s_ref.phase = GcPhase::Pause;
            s_ref.gray_stack.clear();
            s_ref.sweep_ptr = std::ptr::null_mut();
            s_ref.prev_sweep_ptr = std::ptr::null_mut();
            GC_NEEDS_STEP = false;
        });
        gc_clear_string_cache();
    }
}


#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn gc_mark_value(val: &Value) {
    if (val.0 & TAG_NUMBER_MASK) == TAG_NUMBER_MASK {
        let tag = val.0 & 0xffff_0000_0000_0000;
        if (tag >= TAG_STRING && tag <= TAG_FUNCTION) || tag == TAG_METHOD_PUSH || tag == TAG_METHOD_POP {
            let ptr = val.as_gc_ptr();
            unsafe {
                if !ptr.is_null() && (*ptr).color == GcColor::White {
                    (*ptr).color = GcColor::Gray;
                    GC_STATE.with(|state| state.borrow_mut().gray_stack.push(ptr));
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
            GC_STATE.with(|state| state.borrow_mut().gray_stack.push(ptr));
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
            GcData::StructConstructor(_) => {}
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
        if (*parent).color == GcColor::Black {
            if (child.0 & TAG_NUMBER_MASK) == TAG_NUMBER_MASK {
                let tag = child.0 & 0xffff_0000_0000_0000;
                if (tag >= TAG_STRING && tag <= TAG_FUNCTION) || tag == TAG_METHOD_PUSH || tag == TAG_METHOD_POP {
                    let child_ptr = child.as_gc_ptr();
                    if !child_ptr.is_null() && (*child_ptr).color == GcColor::White {
                        (*child_ptr).color = GcColor::Gray;
                        GC_STATE.with(|state| {
                            state.borrow_mut().gray_stack.push(child_ptr);
                        });
                    }
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

pub fn get_or_create_string(s: &str) -> *mut GcObject {
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
            let ptr = get_or_create_string(&s);
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
            let mut map = get_pooled_map(obj.len());
            for (k, v) in obj {
                let key_ptr = get_or_create_string(&k);
                let val = json_to_value(v);
                map.insert(MapKey(Value::string(key_ptr)), val);
            }
            let ptr = gc_allocate(GcData::Object(map));
            Value::object(ptr)
        }
    }
}
