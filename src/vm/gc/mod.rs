#![allow(clippy::not_unsafe_ptr_arg_deref)]

mod types;
mod trace;
mod string_cache;

pub use types::*;
pub use trace::*;
pub use string_cache::*;

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use fnv::FnvHashMap;
use crate::vm::value::{MapKey, Value};

pub type ObjectMap = FnvHashMap<MapKey, Value>;

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
        alloc_threshold: 512,
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
    pub static GC_TEMP_SLICES: RefCell<Vec<(*const Value, usize)>> = RefCell::new(Vec::new());
}

#[inline(always)]
pub fn gc_push_temp_slice(ptr: *const Value, len: usize) {
    GC_TEMP_SLICES.with(|s| {
        s.borrow_mut().push((ptr, len));
    });
}

#[inline(always)]
pub fn gc_pop_temp_slice() {
    GC_TEMP_SLICES.with(|s| {
        s.borrow_mut().pop();
    });
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
            Vec::with_capacity(capacity.max(4))
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
            ObjectMap::with_capacity_and_hasher(capacity.max(4), Default::default())
        }
    })
}

#[inline(always)]
pub fn gc_alloc_array(slice: &[Value]) -> *mut GcObject {
    GC_STATE.with(|state| unsafe {
        let s = &mut *state.get();
        let count = slice.len();
        let cap = if count < 4 { 4 } else { count };
        let mut elements = if let Some(mut v) = s.vector_pool.pop() {
            if v.capacity() < cap {
                v.reserve(cap - v.capacity());
            }
            v.clear();
            v
        } else {
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

const GC_POOL_MAX: usize = 128;

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

const CHUNK_COUNT: usize = 256;

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
        crate::vm::shape::reset_shape_state();
        crate::jit::helpers::reset_global_ic();
        crate::vm::alloc::scavenge_idle_memory();
    }
}
