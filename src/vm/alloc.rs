//! Eronom Memory Allocator Subsystem
//! Unified high-performance memory allocator subsystem using mimalloc with idle scavenging.

pub use mimalloc::MiMalloc;

pub static GLOBAL: MiMalloc = MiMalloc;

/// Proactively scavenge idle memory and return unused pages/capacities to the OS.
/// Called during idle event loop pauses or between batch operations.
pub fn scavenge_idle_memory() {
    // 1. Trim thread-local GC vector and map pools down to baseline
    crate::vm::gc::gc_with_state(|state| {
        if state.vector_pool.len() > 64 {
            state.vector_pool.truncate(64);
            state.vector_pool.shrink_to_fit();
        }
        if state.map_pool.len() > 32 {
            state.map_pool.truncate(32);
            state.map_pool.shrink_to_fit();
        }
        if state.free_list.len() > 256 {
            state.free_list.truncate(256);
            state.free_list.shrink_to_fit();
        }
    });

    // 2. Clear string cache if bloated
    crate::vm::gc::STRING_CACHE.with(|cache| {
        let mut c = cache.borrow_mut();
        if c.len() > 1024 {
            c.shrink_to_fit();
        }
    });
}
