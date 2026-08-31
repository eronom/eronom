//! Eronom Memory Allocator Subsystem
//! Unified high-performance memory allocator subsystem using mimalloc with idle scavenging.

pub use mimalloc::MiMalloc;

pub static GLOBAL: MiMalloc = MiMalloc;

unsafe extern "C" {
    fn mi_collect(force: bool);
    fn mi_option_set(option: i32, val: i64);
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
    // 1. Trim thread-local GC vector and map pools down to baseline
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

