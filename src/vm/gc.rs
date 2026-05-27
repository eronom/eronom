#![allow(clippy::not_unsafe_ptr_arg_deref)]
use super::value::{
    Value, TAG_NUMBER_MASK, TAG_STRING, TAG_FUNCTION, TAG_METHOD_PUSH, TAG_METHOD_POP, MapKey
};
use super::bytecode::Function;
use fnv::FnvHashMap;
use std::rc::Rc;
use std::cell::RefCell;
use indexmap::IndexMap;

pub type ObjectMap = IndexMap<MapKey, Value, fnv::FnvBuildHasher>;

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

pub enum GcData {
    String(Rc<str>),
    Array(Vec<Value>),
    Object(ObjectMap),
    Function(Function),
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
            _ => {
                std::ptr::drop_in_place(data);
            }
        }
        std::ptr::write(data, GcData::String(std::rc::Rc::from("")));
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
            GcData::Function(func) => {
                for constant in &func.chunk.constants {
                    gc_mark_value(constant);
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
