use std::rc::Rc;
use std::cell::RefCell;
use fnv::FnvHashMap;
use crate::vm::value::{Value, MapKey};
use crate::vm::gc::StructDescriptor;

thread_local! {
    // Fast 2-key cache for binary trees {left, right} or 2-property objects {a, b}
    static FAST_2KEY_CACHE: RefCell<Option<(u64, u64, Rc<StructDescriptor>)>> = RefCell::new(None);
    // Fast 1-key cache
    static FAST_1KEY_CACHE: RefCell<Option<(u64, Rc<StructDescriptor>)>> = RefCell::new(None);
    // General shape cache for any key count: key_bits -> (Descriptor, field offsets)
    static SHAPE_CACHE: RefCell<FnvHashMap<Vec<u64>, (Rc<StructDescriptor>, Vec<usize>)>> = RefCell::new(FnvHashMap::default());
    // Transition cache for adding a property: (parent_desc_id, property_key_u64) -> Rc<StructDescriptor>
    static TRANSITION_CACHE: RefCell<FnvHashMap<(u32, u64), Rc<StructDescriptor>>> = RefCell::new(FnvHashMap::default());
}

/// Fetches or creates a cached StructDescriptor for an anonymous object shape with the given keys.
pub fn get_or_create_anonymous_shape(keys: &[Value]) -> (Rc<StructDescriptor>, Vec<usize>) {
    if keys.len() == 2 {
        let k0 = keys[0].0;
        let k1 = keys[1].0;
        let cached = FAST_2KEY_CACHE.with(|c| {
            if let Some((c0, c1, ref desc)) = *c.borrow() {
                if c0 == k0 && c1 == k1 {
                    return Some(desc.clone());
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
            if let Some((c0, ref desc)) = *c.borrow() {
                if c0 == k0 {
                    return Some(desc.clone());
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
                FAST_2KEY_CACHE.with(|f| *f.borrow_mut() = Some((keys[0].0, keys[1].0, entry.0.clone())));
            } else if keys.len() == 1 {
                FAST_1KEY_CACHE.with(|f| *f.borrow_mut() = Some((keys[0].0, entry.0.clone())));
            }
            return entry.clone();
        }

        let mut field_indices = FnvHashMap::default();
        let mut offsets = Vec::with_capacity(keys.len());
        for (i, &key) in keys.iter().enumerate() {
            field_indices.insert(MapKey(key), i);
            offsets.push(i);
        }
        let desc = Rc::new(StructDescriptor::new(
            "Anonymous".into(),
            field_indices,
            FnvHashMap::default(),
        ));
        let entry = (desc.clone(), offsets);
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
pub fn transition_shape_add_property(parent: &Rc<StructDescriptor>, new_key: Value) -> Rc<StructDescriptor> {
    let key_tuple = (parent.id, new_key.0);
    TRANSITION_CACHE.with(|cache| {
        let mut c = cache.borrow_mut();
        if let Some(desc) = c.get(&key_tuple) {
            return desc.clone();
        }
        let mut new_indices = parent.field_indices.clone();
        let new_idx = new_indices.len();
        new_indices.insert(MapKey(new_key), new_idx);
        let new_desc = Rc::new(StructDescriptor::new(
            "Anonymous".into(),
            new_indices,
            FnvHashMap::default(),
        ));
        c.insert(key_tuple, new_desc.clone());
        new_desc
    })
}

/// Resets shape state across hot reloads.
pub fn reset_shape_state() {
    FAST_2KEY_CACHE.with(|c| *c.borrow_mut() = None);
    FAST_1KEY_CACHE.with(|c| *c.borrow_mut() = None);
    SHAPE_CACHE.with(|c| c.borrow_mut().clear());
    TRANSITION_CACHE.with(|c| c.borrow_mut().clear());
}
