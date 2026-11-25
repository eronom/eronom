//! Eronom Memory Allocator Subsystem
//! Unified high-performance memory allocator subsystem using mimalloc,
//! custom Allocator API slab pools, and bump arenas with idle scavenging.

use std::alloc::{AllocError, Allocator, GlobalAlloc, Layout};
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

const SLAB_BUCKET_COUNT: usize = 8; // 32, 64, 128, 256, 512, 1024, 2048, 4096
const SLAB_MAX_SIZE: usize = 4096;
const SLAB_BUCKET_CAP: usize = 256;

struct SlabPoolState {
    buckets: [Vec<*mut u8>; SLAB_BUCKET_COUNT],
}

impl SlabPoolState {
    const fn new() -> Self {
        Self {
            buckets: [const { Vec::new() }; SLAB_BUCKET_COUNT],
        }
    }

    fn scavenge(&mut self, keep_per_bucket: usize) {
        for bucket in &mut self.buckets {
            while bucket.len() > keep_per_bucket {
                if let Some(ptr) = bucket.pop() {
                    unsafe { mi_free(ptr) };
                }
            }
            if keep_per_bucket == 0 {
                bucket.shrink_to_fit();
            }
        }
    }
}

impl Drop for SlabPoolState {
    fn drop(&mut self) {
        self.scavenge(0);
    }
}

thread_local! {
    static GC_SLAB_POOL: UnsafeCell<SlabPoolState> = const { UnsafeCell::new(SlabPoolState::new()) };
}

#[inline(always)]
fn slab_bucket_for(size: usize) -> Option<(usize, usize)> {
    if size > SLAB_MAX_SIZE {
        return None;
    }
    if size <= 32 {
        Some((0, 32))
    } else if size <= 64 {
        Some((1, 64))
    } else if size <= 128 {
        Some((2, 128))
    } else if size <= 256 {
        Some((3, 256))
    } else if size <= 512 {
        Some((4, 512))
    } else if size <= 1024 {
        Some((5, 1024))
    } else if size <= 2048 {
        Some((6, 2048))
    } else {
        Some((7, 4096))
    }
}

#[inline(always)]
fn dangling_for_layout(layout: Layout) -> NonNull<[u8]> {
    let ptr = std::ptr::without_provenance_mut::<u8>(layout.align());
    // SAFETY: Layout alignment is always non-zero.
    let nn = unsafe { NonNull::new_unchecked(ptr) };
    NonNull::slice_from_raw_parts(nn, 0)
}

/// Thread-local size-classed slab allocator backed by mimalloc for GC collections.
#[derive(Copy, Clone, Default, Debug)]
pub struct GcSlabAlloc;

pub type GcVec<T> = Vec<T, GcSlabAlloc>;
pub type GcBox<T> = Box<T, GcSlabAlloc>;

impl GcSlabAlloc {
    #[inline]
    fn alloc_raw(&self, layout: Layout, zeroed: bool) -> Result<NonNull<[u8]>, AllocError> {
        let size = layout.size();
        if size == 0 {
            return Ok(dangling_for_layout(layout));
        }
        if layout.align() <= 16 {
            if let Some((idx, bucket_size)) = slab_bucket_for(size) {
                let cached = GC_SLAB_POOL.with(|pool| unsafe { (*pool.get()).buckets[idx].pop() });
                if let Some(ptr) = cached {
                    if zeroed {
                        unsafe { std::ptr::write_bytes(ptr, 0, bucket_size) };
                    }
                    let nn = unsafe { NonNull::new_unchecked(ptr) };
                    return Ok(NonNull::slice_from_raw_parts(nn, bucket_size));
                }
                let raw = unsafe {
                    if zeroed {
                        mi_zalloc_aligned(bucket_size, 16)
                    } else {
                        mi_malloc_aligned(bucket_size, 16)
                    }
                };
                let nn = NonNull::new(raw).ok_or(AllocError)?;
                return Ok(NonNull::slice_from_raw_parts(nn, bucket_size));
            }
        }
        let align = layout.align().max(16);
        let raw = unsafe {
            if zeroed {
                mi_zalloc_aligned(size, align)
            } else {
                mi_malloc_aligned(size, align)
            }
        };
        let nn = NonNull::new(raw).ok_or(AllocError)?;
        Ok(NonNull::slice_from_raw_parts(nn, size))
    }
}

unsafe impl Allocator for GcSlabAlloc {
    #[inline]
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        self.alloc_raw(layout, false)
    }

    #[inline]
    fn allocate_zeroed(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        self.alloc_raw(layout, true)
    }

    #[inline]
    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        let size = layout.size();
        if size == 0 {
            return;
        }
        if layout.align() <= 16 {
            if let Some((idx, _)) = slab_bucket_for(size) {
                let pushed = GC_SLAB_POOL.with(|pool| unsafe {
                    let b = &mut (*pool.get()).buckets[idx];
                    if b.len() < SLAB_BUCKET_CAP {
                        b.push(ptr.as_ptr());
                        true
                    } else {
                        false
                    }
                });
                if pushed {
                    return;
                }
            }
        }
        unsafe { mi_free(ptr.as_ptr()) };
    }

    #[inline]
    unsafe fn grow(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<NonNull<[u8]>, AllocError> {
        let old_size = old_layout.size();
        let new_size = new_layout.size();
        if old_size == 0 {
            return self.allocate(new_layout);
        }
        if old_layout.align() <= 16 && new_layout.align() <= 16 {
            if let (Some((old_idx, _)), Some((new_idx, new_bucket_size))) =
                (slab_bucket_for(old_size), slab_bucket_for(new_size))
            {
                if old_idx == new_idx {
                    return Ok(NonNull::slice_from_raw_parts(ptr, new_bucket_size));
                }
            } else if slab_bucket_for(old_size).is_none() && slab_bucket_for(new_size).is_none() {
                let raw = unsafe { mi_realloc_aligned(ptr.as_ptr(), new_size, 16) };
                let nn = NonNull::new(raw).ok_or(AllocError)?;
                return Ok(NonNull::slice_from_raw_parts(nn, new_size));
            }
        }
        let new_ptr = self.allocate(new_layout)?;
        unsafe {
            std::ptr::copy_nonoverlapping(ptr.as_ptr(), new_ptr.as_ptr() as *mut u8, old_size);
            self.deallocate(ptr, old_layout);
        }
        Ok(new_ptr)
    }

    #[inline]
    unsafe fn grow_zeroed(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<NonNull<[u8]>, AllocError> {
        let old_size = old_layout.size();
        let new_ptr = unsafe { self.grow(ptr, old_layout, new_layout)? };
        let allocated_len = new_ptr.len();
        if allocated_len > old_size {
            unsafe {
                std::ptr::write_bytes(
                    (new_ptr.as_ptr() as *mut u8).add(old_size),
                    0,
                    allocated_len - old_size,
                );
            }
        }
        Ok(new_ptr)
    }

    #[inline]
    unsafe fn shrink(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<NonNull<[u8]>, AllocError> {
        let new_size = new_layout.size();
        if new_size == 0 {
            unsafe { self.deallocate(ptr, old_layout) };
            return Ok(dangling_for_layout(new_layout));
        }
        if old_layout.align() <= 16 && new_layout.align() <= 16 {
            if let (Some((old_idx, _)), Some((new_idx, new_bucket_size))) =
                (slab_bucket_for(old_layout.size()), slab_bucket_for(new_size))
            {
                if old_idx == new_idx {
                    return Ok(NonNull::slice_from_raw_parts(ptr, new_bucket_size));
                }
            }
        }
        let new_ptr = self.allocate(new_layout)?;
        unsafe {
            std::ptr::copy_nonoverlapping(ptr.as_ptr(), new_ptr.as_ptr() as *mut u8, new_size);
            self.deallocate(ptr, old_layout);
        }
        Ok(new_ptr)
    }
}

/// Allocate an UTF-8 string slice inside `GcSlabAlloc`.
#[inline]
pub fn alloc_box_str_in_slab(s: &str) -> GcBox<str> {
    let len = s.len();
    if len == 0 {
        let empty = dangling_for_layout(Layout::new::<u8>()).as_ptr() as *mut u8;
        let str_ptr = std::ptr::slice_from_raw_parts_mut(empty, 0) as *mut str;
        return unsafe { Box::from_raw_in(str_ptr, GcSlabAlloc) };
    }
    let layout = Layout::array::<u8>(len).expect("string layout overflow");
    let raw = GcSlabAlloc
        .allocate(layout)
        .expect("slab allocation failed")
        .as_ptr() as *mut u8;
    unsafe {
        std::ptr::copy_nonoverlapping(s.as_ptr(), raw, len);
        let str_ptr = std::ptr::slice_from_raw_parts_mut(raw, len) as *mut str;
        Box::from_raw_in(str_ptr, GcSlabAlloc)
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

    fn allocate_bytes(&mut self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        let size = layout.size();
        if size == 0 {
            return Ok(dangling_for_layout(layout));
        }
        let align = layout.align();
        if size > SHAPE_ARENA_SLAB_SIZE / 4 || align > 16 {
            let alloc_align = align.max(16);
            let raw = unsafe { mi_malloc_aligned(size, alloc_align) };
            let nn = NonNull::new(raw).ok_or(AllocError)?;
            self.slabs.push(raw);
            return Ok(NonNull::slice_from_raw_parts(nn, size));
        }

        let aligned = (self.cursor + align - 1) & !(align - 1);
        if let Some(next_cursor) = aligned.checked_add(size) {
            if next_cursor <= self.end {
                self.cursor = next_cursor;
                let ptr = std::ptr::with_exposed_provenance_mut::<u8>(aligned);
                let nn = unsafe { NonNull::new_unchecked(ptr) };
                return Ok(NonNull::slice_from_raw_parts(nn, size));
            }
        }

        let slab = unsafe { mi_malloc_aligned(SHAPE_ARENA_SLAB_SIZE, 16) };
        if slab.is_null() {
            return Err(AllocError);
        }
        self.slabs.push(slab);
        let base = slab.expose_provenance();
        let aligned_new = (base + align - 1) & !(align - 1);
        self.cursor = aligned_new + size;
        self.end = base + SHAPE_ARENA_SLAB_SIZE;
        let ptr = std::ptr::with_exposed_provenance_mut::<u8>(aligned_new);
        let nn = unsafe { NonNull::new_unchecked(ptr) };
        Ok(NonNull::slice_from_raw_parts(nn, size))
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

/// Thread-local bump arena allocator for immutable runtime shape metadata.
#[derive(Copy, Clone, Default, Debug)]
pub struct ShapeArenaAlloc;

pub type ShapeBox<T> = Box<T, ShapeArenaAlloc>;

unsafe impl Allocator for ShapeArenaAlloc {
    #[inline]
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        SHAPE_ARENA.with(|arena| unsafe { (*arena.get()).allocate_bytes(layout) })
    }

    #[inline]
    unsafe fn deallocate(&self, _ptr: NonNull<u8>, _layout: Layout) {
        // Bulk-reclaimed when reset_shape_arena() drops the arena slabs.
    }
}

/// Allocate an UTF-8 string slice inside `ShapeArenaAlloc`.
#[inline]
pub fn alloc_box_str_in_shape_arena(s: &str) -> ShapeBox<str> {
    let len = s.len();
    if len == 0 {
        let empty = dangling_for_layout(Layout::new::<u8>()).as_ptr() as *mut u8;
        let str_ptr = std::ptr::slice_from_raw_parts_mut(empty, 0) as *mut str;
        return unsafe { Box::from_raw_in(str_ptr, ShapeArenaAlloc) };
    }
    let layout = Layout::array::<u8>(len).expect("string layout overflow");
    let raw = ShapeArenaAlloc
        .allocate(layout)
        .expect("shape arena allocation failed")
        .as_ptr() as *mut u8;
    unsafe {
        std::ptr::copy_nonoverlapping(s.as_ptr(), raw, len);
        let str_ptr = std::ptr::slice_from_raw_parts_mut(raw, len) as *mut str;
        Box::from_raw_in(str_ptr, ShapeArenaAlloc)
    }
}

/// Allocate `val` inside `ShapeArenaAlloc` and register its destructor to run on arena reset.
pub fn alloc_in_shape_arena<T>(val: T) -> *const T {
    unsafe fn drop_t<T>(ptr: *mut u8) {
        unsafe { std::ptr::drop_in_place(ptr as *mut T) };
    }
    let boxed = Box::new_in(val, ShapeArenaAlloc);
    let (ptr, _) = Box::into_raw_with_allocator(boxed);
    SHAPE_ARENA.with(|arena| unsafe {
        (*arena.get()).destructors.push((ptr as *mut u8, drop_t::<T>));
    });
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
    // 1. Trim thread-local GC map pool and free list down to baseline
    crate::vm::gc::gc_with_state(|state| {
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

    GC_SLAB_POOL.with(|pool| unsafe { (*pool.get()).scavenge(16) });

    collect_allocator_memory(true);
}

use crate::vm::value::Value;
use crate::vm::gc::{gc_allocate, get_pooled_vec, GcData, GcStruct, ShapePtr};

pub fn construct_struct_from_args(
    descriptor: ShapePtr,
    args: Vec<Value>,
) -> Result<Value, String> {
    if args.is_empty() {
        let count = descriptor.field_indices.len();
        let mut fields = get_pooled_vec(count);
        fields.resize(count, Value::null());
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
                        let mut fields = get_pooled_vec(count);
                        fields.resize(count, Value::null());
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
                        let mut fields = get_pooled_vec(count);
                        fields.resize(count, Value::null());
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
    let mut fields = get_pooled_vec(count);
    fields.resize(count, Value::null());
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
    fn test_gc_slab_alloc_vec_growth_and_shrink() {
        let mut v: GcVec<u64> = Vec::new_in(GcSlabAlloc);
        for i in 0..512u64 {
            v.push(i * 3);
        }
        assert_eq!(v.len(), 512);
        assert_eq!(v[255], 765);
        v.truncate(4);
        v.shrink_to_fit();
        assert_eq!(v.len(), 4);
        assert_eq!(v[3], 9);

        let s = alloc_box_str_in_slab("hello_eronom_allocator_api_long_string_buffer");
        assert_eq!(&*s, "hello_eronom_allocator_api_long_string_buffer");
        let empty = alloc_box_str_in_slab("");
        assert_eq!(&*empty, "");

        GC_SLAB_POOL.with(|pool| unsafe { (*pool.get()).scavenge(0) });
    }

    #[test]
    fn test_gc_slab_alloc_alignment_and_zeroed() {
        #[repr(align(64))]
        struct Aligned64([u64; 8]);

        let b = Box::new_in(Aligned64([7; 8]), GcSlabAlloc);
        assert_eq!(((&*b) as *const Aligned64 as usize) % 64, 0);
        assert_eq!(b.0[7], 7);

        let layout = Layout::from_size_align(128, 32).unwrap();
        let mem = GcSlabAlloc.allocate_zeroed(layout).unwrap();
        unsafe {
            let slice = std::slice::from_raw_parts(mem.as_ptr() as *const u8, 128);
            assert!(slice.iter().all(|&byte| byte == 0));
            GcSlabAlloc.deallocate(mem.cast(), layout);
        }
    }

    #[test]
    fn test_shape_arena_alloc_and_reset() {
        reset_shape_arena();
        let b1 = Box::new_in(vec![1u64, 2, 3, 4], ShapeArenaAlloc);
        let s1 = alloc_box_str_in_shape_arena("PlayerTransform");
        assert_eq!(&*b1, &[1, 2, 3, 4]);
        assert_eq!(&*s1, "PlayerTransform");

        let (p1, _) = Box::into_raw_with_allocator(b1);
        let (p_s1, _) = Box::into_raw_with_allocator(s1);
        unsafe {
            std::ptr::drop_in_place(p1);
            std::ptr::drop_in_place(p_s1);
        }
        reset_shape_arena();
    }
}

