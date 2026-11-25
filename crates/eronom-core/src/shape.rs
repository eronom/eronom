use std::cell::RefCell;
use fnv::FnvHashMap;
use crate::vm::value::{Value, MapKey};
use crate::vm::gc::{ShapePtr, StructDescriptor};

thread_local! {
    // Fast 2-key cache for binary trees {left, right} or 2-property objects {a, b}
    static FAST_2KEY_CACHE: RefCell<Option<(u64, u64, ShapePtr)>> = RefCell::new(None);
    // Fast 1-key cache
    static FAST_1KEY_CACHE: RefCell<Option<(u64, ShapePtr)>> = RefCell::new(None);
    // General shape cache for any key count: key_bits -> (Descriptor, field offsets)
    static SHAPE_CACHE: RefCell<FnvHashMap<Vec<u64>, (ShapePtr, Vec<usize>)>> = RefCell::new(FnvHashMap::default());
    // Transition cache for adding a property: (parent_desc_id, property_key_u64) -> ShapePtr
    static TRANSITION_CACHE: RefCell<FnvHashMap<(u32, u64), ShapePtr>> = RefCell::new(FnvHashMap::default());
}

#[inline]
pub fn alloc_shape_descriptor(desc: StructDescriptor) -> ShapePtr {
    ShapePtr(crate::alloc::alloc_in_shape_arena(desc))
}

/// Fast-path 2-key shape lookup avoiding allocations
#[inline(always)]
pub fn get_or_create_anonymous_shape_2(k0: Value, k1: Value) -> (ShapePtr, [usize; 2]) {
    let k0_bits = k0.0;
    let k1_bits = k1.0;
    let cached = FAST_2KEY_CACHE.with(|c| {
        if let Some((c0, c1, desc)) = *c.borrow() {
            if c0 == k0_bits && c1 == k1_bits {
                return Some(desc);
            }
        }
        None
    });
    if let Some(desc) = cached {
        return (desc, [0, 1]);
    }
    let (desc, offsets) = get_or_create_anonymous_shape(&[k0, k1]);
    (desc, [offsets[0], offsets[1]])
}

/// Fetches or creates a cached StructDescriptor for an anonymous object shape with the given keys.
pub fn get_or_create_anonymous_shape(keys: &[Value]) -> (ShapePtr, Vec<usize>) {
    if keys.len() == 2 {
        let k0 = keys[0].0;
        let k1 = keys[1].0;
        let cached = FAST_2KEY_CACHE.with(|c| {
            if let Some((c0, c1, desc)) = *c.borrow() {
                if c0 == k0 && c1 == k1 {
                    return Some(desc);
                }
            }
            None
        });
        if let Some(desc) = cached {
            return (desc, vec![0, 1]);
        }
    } else if keys.len() == 1 {
        let k0 = keys[0].0;
        let cached = FAST_1KEY_CACHE.with(|c| {
            if let Some((c0, desc)) = *c.borrow() {
                if c0 == k0 {
                    return Some(desc);
                }
            }
            None
        });
        if let Some(desc) = cached {
            return (desc, vec![0]);
        }
    }

    let key_bits: Vec<u64> = keys.iter().map(|k| k.0).collect();
    SHAPE_CACHE.with(|cache| {
        let mut c = cache.borrow_mut();
        if let Some(entry) = c.get(&key_bits) {
            if keys.len() == 2 {
                FAST_2KEY_CACHE.with(|f| *f.borrow_mut() = Some((keys[0].0, keys[1].0, entry.0)));
            } else if keys.len() == 1 {
                FAST_1KEY_CACHE.with(|f| *f.borrow_mut() = Some((keys[0].0, entry.0)));
            }
            return entry.clone();
        }

        let mut field_indices = FnvHashMap::default();
        let mut offsets = Vec::with_capacity(keys.len());
        for (i, &key) in keys.iter().enumerate() {
            field_indices.insert(MapKey(key), i);
            offsets.push(i);
        }
        let desc = alloc_shape_descriptor(StructDescriptor::new(
            "Anonymous",
            field_indices,
            FnvHashMap::default(),
        ));
        let entry = (desc, offsets);
        c.insert(key_bits, entry.clone());
        if keys.len() == 2 {
            FAST_2KEY_CACHE.with(|f| *f.borrow_mut() = Some((keys[0].0, keys[1].0, desc)));
        } else if keys.len() == 1 {
            FAST_1KEY_CACHE.with(|f| *f.borrow_mut() = Some((keys[0].0, desc)));
        }
        entry
    })
}

/// Dynamically transitions an anonymous object shape when a new property is set.
pub fn transition_shape_add_property(parent: ShapePtr, new_key: Value) -> ShapePtr {
    let key_tuple = (parent.id, new_key.0);
    TRANSITION_CACHE.with(|cache| {
        let mut c = cache.borrow_mut();
        if let Some(&desc) = c.get(&key_tuple) {
            return desc;
        }
        let mut new_indices = parent.field_indices.clone();
        let new_idx = new_indices.len();
        new_indices.insert(MapKey(new_key), new_idx);
        let new_desc = alloc_shape_descriptor(StructDescriptor::new(
            "Anonymous",
            new_indices,
            FnvHashMap::default(),
        ));
        c.insert(key_tuple, new_desc);
        new_desc
    })
}

/// Resets shape state across hot reloads.
pub fn reset_shape_state() {
    FAST_2KEY_CACHE.with(|c| *c.borrow_mut() = None);
    FAST_1KEY_CACHE.with(|c| *c.borrow_mut() = None);
    SHAPE_CACHE.with(|c| c.borrow_mut().clear());
    TRANSITION_CACHE.with(|c| c.borrow_mut().clear());
    crate::alloc::reset_shape_arena();
}
