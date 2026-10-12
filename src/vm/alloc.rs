//! Eronom Memory Allocator Subsystem
//! Unified high-performance memory allocator subsystem using mimalloc with idle scavenging.

pub use mimalloc::MiMalloc;

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

use crate::vm::value::Value;
use crate::vm::gc::{gc_allocate, get_pooled_vec, GcData, GcStruct, StructDescriptor};

pub fn construct_struct_from_args(
    descriptor: &std::rc::Rc<StructDescriptor>,
    args: Vec<Value>,
) -> Result<Value, String> {
    if args.is_empty() {
        let count = descriptor.field_indices.len();
        let mut fields = get_pooled_vec(count);
        fields.resize(count, Value::null());
        let s_ptr = gc_allocate(GcData::Struct(GcStruct {
            descriptor: descriptor.clone(),
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
                            descriptor: descriptor.clone(),
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
                            descriptor: descriptor.clone(),
                            fields,
                        }));
                        return Ok(Value::object(s_ptr));
                    }
                    _ => {}
                }
            }
        } else if arg.is_array() {
            let arg_ptr = arg.as_gc_ptr();
            let mut mapped_elements = Vec::new();
            unsafe {
                if let GcData::Array(arr) = &(*arg_ptr).data {
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
        descriptor: descriptor.clone(),
        fields,
    }));
    Ok(Value::object(s_ptr))
}

