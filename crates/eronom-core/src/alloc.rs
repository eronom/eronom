//! Eronom Memory Allocator Subsystem
//! Unified high-performance memory allocator subsystem using mimalloc,
//! thread-local GC pools, and a bump arena for shape metadata with idle scavenging.

use std::alloc::{GlobalAlloc, Layout};
use std::cell::UnsafeCell;
use std::ptr::NonNull;

unsafe extern "C" {
    pub fn mi_malloc(size: usize) -> *mut u8;
    pub fn mi_malloc_aligned(size: usize, alignment: usize) -> *mut u8;
    pub fn mi_free(p: *mut u8);
    pub fn mi_realloc(p: *mut u8, newsize: usize) -> *mut u8;
    pub fn mi_realloc_aligned(p: *mut u8, newsize: usize, alignment: usize) -> *mut u8;
    pub fn mi_zalloc(size: usize) -> *mut u8;
    pub fn mi_zalloc_aligned(size: usize, alignment: usize) -> *mut u8;
    pub fn mi_collect(force: bool);
    pub fn mi_option_set(option: i32, val: i64);
}

#[derive(Copy, Clone, Default, Debug)]
pub struct MiMalloc;

unsafe impl GlobalAlloc for MiMalloc {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe {
            if layout.align() <= 16 {
                mi_malloc(layout.size())
            } else {
                mi_malloc_aligned(layout.size(), layout.align())
            }
        }
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        unsafe { mi_free(ptr) }
    }

    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe {
            if layout.align() <= 16 {
                mi_realloc(ptr, new_size)
            } else {
                mi_realloc_aligned(ptr, new_size, layout.align())
            }
        }
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        unsafe {
            if layout.align() <= 16 {
                mi_zalloc(layout.size())
            } else {
                mi_zalloc_aligned(layout.size(), layout.align())
            }
        }
    }
}

const SHAPE_ARENA_SLAB_SIZE: usize = 64 * 1024;

struct ShapeArenaState {
    slabs: Vec<*mut u8>,
    cursor: usize,
    end: usize,
    destructors: Vec<(*mut u8, unsafe fn(*mut u8))>,
}

impl ShapeArenaState {
    const fn new() -> Self {
        Self {
            slabs: Vec::new(),
            cursor: 0,
            end: 0,
            destructors: Vec::new(),
        }
    }

    fn allocate_bytes(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        let size = layout.size();
        let align = layout.align();
        if size == 0 {
            let ptr = std::ptr::without_provenance_mut::<u8>(align);
            return unsafe { Some(NonNull::new_unchecked(ptr)) };
        }
        if size > SHAPE_ARENA_SLAB_SIZE / 4 || align > 16 {
            let alloc_align = align.max(16);
            let raw = unsafe { mi_malloc_aligned(size, alloc_align) };
            let nn = NonNull::new(raw)?;
            self.slabs.push(raw);
            return Some(nn);
        }

        let aligned = (self.cursor + align - 1) & !(align - 1);
        if let Some(next_cursor) = aligned.checked_add(size) {
            if next_cursor <= self.end {
                self.cursor = next_cursor;
                let ptr = std::ptr::with_exposed_provenance_mut::<u8>(aligned);
                return unsafe { Some(NonNull::new_unchecked(ptr)) };
            }
        }

        let slab = unsafe { mi_malloc_aligned(SHAPE_ARENA_SLAB_SIZE, 16) };
        if slab.is_null() {
            return None;
        }
        self.slabs.push(slab);
        let base = slab.expose_provenance();
        let aligned_new = (base + align - 1) & !(align - 1);
        self.cursor = aligned_new + size;
        self.end = base + SHAPE_ARENA_SLAB_SIZE;
        let ptr = std::ptr::with_exposed_provenance_mut::<u8>(aligned_new);
        unsafe { Some(NonNull::new_unchecked(ptr)) }
    }

    fn reset(&mut self) {
        while let Some((ptr, drop_fn)) = self.destructors.pop() {
            unsafe { drop_fn(ptr) };
        }
        for slab in self.slabs.drain(..) {
            unsafe { mi_free(slab) };
        }
        self.cursor = 0;
        self.end = 0;
    }
}

impl Drop for ShapeArenaState {
    fn drop(&mut self) {
        self.reset();
    }
}

thread_local! {
    static SHAPE_ARENA: UnsafeCell<ShapeArenaState> = const { UnsafeCell::new(ShapeArenaState::new()) };
}

/// Immutable UTF-8 string slice stored inside the thread-local shape bump arena.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ShapeStr {
    ptr: *const u8,
    len: usize,
}

unsafe impl Send for ShapeStr {}
unsafe impl Sync for ShapeStr {}

impl ShapeStr {
    #[inline(always)]
    pub fn as_str(&self) -> &str {
        if self.len == 0 {
            ""
        } else {
            unsafe {
                let bytes = std::slice::from_raw_parts(self.ptr, self.len);
                std::str::from_utf8_unchecked(bytes)
            }
        }
    }
}

impl AsRef<str> for ShapeStr {
    #[inline(always)]
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::ops::Deref for ShapeStr {
    type Target = str;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl std::fmt::Debug for ShapeStr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.as_str(), f)
    }
}

/// Copy an UTF-8 string slice into the thread-local shape bump arena.
#[inline]
pub fn alloc_str_in_shape_arena(s: &str) -> ShapeStr {
    let len = s.len();
    if len == 0 {
        return ShapeStr {
            ptr: std::ptr::NonNull::<u8>::dangling().as_ptr(),
            len: 0,
        };
    }
    let layout = Layout::array::<u8>(len).expect("string layout overflow");
    let raw = SHAPE_ARENA
        .with(|arena| unsafe { (*arena.get()).allocate_bytes(layout) })
        .expect("shape arena allocation failed")
        .as_ptr();
    unsafe {
        std::ptr::copy_nonoverlapping(s.as_ptr(), raw, len);
    }
    ShapeStr { ptr: raw, len }
}

/// Allocate `val` inside `SHAPE_ARENA` and register its destructor to run on arena reset.
pub fn alloc_in_shape_arena<T>(val: T) -> *const T {
    unsafe fn drop_t<T>(ptr: *mut u8) {
        unsafe { std::ptr::drop_in_place(ptr as *mut T) };
    }
    let layout = Layout::new::<T>();
    let nn = SHAPE_ARENA
        .with(|arena| unsafe { (*arena.get()).allocate_bytes(layout) })
        .expect("shape arena allocation failed");
    let ptr = nn.as_ptr();
    unsafe {
        ptr.cast::<T>().write(val);
        SHAPE_ARENA.with(|arena| {
            (*arena.get()).destructors.push((ptr, drop_t::<T>));
        });
    }
    ptr as *const T
}

/// Reclaim all shape arena bump slabs. Caller must ensure all shape users have been destroyed.
pub fn reset_shape_arena() {
    SHAPE_ARENA.with(|arena| unsafe { (*arena.get()).reset() });
}

use std::sync::Once;
static INIT_ALLOC: Once = Once::new();

/// Configure mimalloc for minimal resident footprint and immediate page purging.
pub fn init_allocator_options() {
    INIT_ALLOC.call_once(|| unsafe {
        // mi_option_eager_commit = 3 (0 = lazy on-demand page commits)
        mi_option_set(3, 0);
        // mi_option_arena_eager_commit = 4 (0 = lazy arena commits)
        mi_option_set(4, 0);
        // mi_option_purge_delay = 7 (0 = immediately purge free pages)
        mi_option_set(7, 0);
        // mi_option_arena_reserve = 12 (0 = disable large virtual arena reserve)
        mi_option_set(12, 0);
        // mi_option_reset_delay = 8
        mi_option_set(8, 0);
        // mi_option_reset_decommits = 9
        mi_option_set(9, 1);
        // mi_option_purge_extend_delay = 19
        mi_option_set(19, 0);
    });
}

/// Proactively trigger mimalloc page collection and return memory to the OS.
pub fn collect_allocator_memory(force: bool) {
    unsafe {
        mi_collect(force);
    }
}

/// Proactively scavenge idle memory and return unused pages/capacities to the OS.
/// Called during idle event loop pauses or between batch operations.
pub fn scavenge_idle_memory() {
    // 1. Trim thread-local GC vector/map pools and free list down to baseline
    crate::vm::gc::gc_with_state(|state| {
        if state.vector_pool.len() > 32 {
            state.vector_pool.truncate(32);
            state.vector_pool.shrink_to_fit();
        }
        if state.map_pool.len() > 16 {
            state.map_pool.truncate(16);
            state.map_pool.shrink_to_fit();
        }
        if state.free_list.len() > 64 {
            state.free_list.truncate(64);
            state.free_list.shrink_to_fit();
        }
    });

    // 2. Clear string cache if bloated
    crate::vm::gc::STRING_CACHE.with(|cache| {
        let mut c = cache.borrow_mut();
        if c.len() > 256 {
            c.shrink_to_fit();
        }
    });

    collect_allocator_memory(true);
}

use crate::vm::value::Value;
use crate::vm::gc::{gc_allocate, GcData, GcStruct, ShapePtr, StructFields};

pub fn construct_struct_from_args(
    descriptor: ShapePtr,
    args: Vec<Value>,
) -> Result<Value, String> {
    if args.is_empty() {
        let count = descriptor.field_indices.len();
        let fields = StructFields::with_len_null(count);
        let s_ptr = gc_allocate(GcData::Struct(GcStruct {
            descriptor,
            fields,
        }));
        return Ok(Value::object(s_ptr));
    }

    if args.len() == 1 {
        let arg = args[0];
        if arg.is_object() {
            let arg_ptr = arg.as_gc_ptr();
            unsafe {
                match &(*arg_ptr).data {
                    GcData::Object(map) => {
                        let count = descriptor.field_indices.len();
                        let mut fields = StructFields::with_len_null(count);
                        for (map_key, &idx) in &descriptor.field_indices {
                            if let Some(&val) = map.get(map_key) {
                                fields[idx] = val;
                            }
                        }
                        let s_ptr = gc_allocate(GcData::Struct(GcStruct {
                            descriptor,
                            fields,
                        }));
                        return Ok(Value::object(s_ptr));
                    }
                    GcData::Struct(s) => {
                        let count = descriptor.field_indices.len();
                        let mut fields = StructFields::with_len_null(count);
                        for (map_key, &idx) in &descriptor.field_indices {
                            if let Some(val) = s.get_field(map_key.0) {
                                fields[idx] = val;
                            }
                        }
                        let s_ptr = gc_allocate(GcData::Struct(GcStruct {
                            descriptor,
                            fields,
                        }));
                        return Ok(Value::object(s_ptr));
                    }
                    _ => {}
                }
            }
        } else if arg.is_array() {
            let arg_ptr = arg.as_gc_ptr();
            let mut mapped_elements = crate::vm::gc::gc_vec_new();
            unsafe {
                if let GcData::Array(arr) = &(*arg_ptr).data {
                    mapped_elements.reserve(arr.len());
                    for &item in arr {
                        let constructed = construct_struct_from_args(descriptor, vec![item])?;
                        mapped_elements.push(constructed);
                    }
                }
            }
            let array_ptr = gc_allocate(GcData::Array(mapped_elements));
            return Ok(Value::array(array_ptr));
        }
    }

    let count = descriptor.field_indices.len();
    let mut fields = StructFields::with_len_null(count);
    for i in 0..args.len().min(count) {
        fields[i] = args[i];
    }
    let s_ptr = gc_allocate(GcData::Struct(GcStruct {
        descriptor,
        fields,
    }));
    Ok(Value::object(s_ptr))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gc_string_and_vec_allocation() {
        let mut v: Vec<u64> = Vec::new();
        for i in 0..512u64 {
            v.push(i * 3);
        }
        assert_eq!(v.len(), 512);
        assert_eq!(v[255], 765);
        v.truncate(4);
        v.shrink_to_fit();
        assert_eq!(v.len(), 4);
        assert_eq!(v[3], 9);

        let s: Box<str> = Box::from("hello_eronom_long_heap_string_buffer_no_rc");
        assert_eq!(&*s, "hello_eronom_long_heap_string_buffer_no_rc");
        let empty: Box<str> = Box::from("");
        assert_eq!(&*empty, "");
    }

    #[test]
    fn test_shape_arena_alloc_and_reset() {
        reset_shape_arena();
        let ptr = alloc_in_shape_arena(vec![1u64, 2, 3, 4]);
        let s1 = alloc_str_in_shape_arena("PlayerTransform");
        let empty = alloc_str_in_shape_arena("");
        unsafe {
            assert_eq!(&*ptr, &[1, 2, 3, 4]);
        }
        assert_eq!(&*s1, "PlayerTransform");
        assert_eq!(&*empty, "");
        reset_shape_arena();
    }
}

