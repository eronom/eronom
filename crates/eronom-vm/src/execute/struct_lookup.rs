use fnv::FnvHashMap;
use crate::vm::value::{Value, MapKey};
use crate::vm::gc::{ShapePtr, StructDescriptor};
use super::types::{VM, ShapeRegistry};

impl ShapeRegistry {
    pub fn find_matching_struct(&self, keys: &[Value]) -> Option<ShapePtr> {
        for &desc in self.structs.values() {
            if desc.field_indices.len() == keys.len() {
                let mut all_match = true;
                for &key in keys {
                    if !desc.field_indices.contains_key(&MapKey(key)) {
                        all_match = false;
                        break;
                    }
                }
                if all_match {
                    return Some(desc);
                }
            }
        }
        None
    }

    pub fn find_matching_struct_cached(&mut self, keys: &[Value]) -> Option<(ShapePtr, &[usize])> {
        if keys.is_empty() {
            return None;
        }
        if let Some(last_desc) = self.last_matched_descriptor {
            if self.last_matched_keys.len() == keys.len() {
                let mut match_ok = true;
                for i in 0..keys.len() {
                    if self.last_matched_keys[i].0 != keys[i].0 {
                        match_ok = false;
                        break;
                    }
                }
                if match_ok {
                    return Some((last_desc, &self.last_matched_offsets));
                }
            }
        }

        for &desc in self.structs.values() {
            if desc.field_indices.len() == keys.len() {
                let mut all_match = true;
                let mut offsets = Vec::with_capacity(keys.len());
                for &key in keys {
                    if let Some(&idx) = desc.field_indices.get(&MapKey(key)) {
                        offsets.push(idx);
                    } else {
                        all_match = false;
                        break;
                    }
                }
                if all_match {
                    self.last_matched_keys = keys.to_vec();
                    self.last_matched_descriptor = Some(desc);
                    self.last_matched_offsets = offsets;
                    return Some((desc, &self.last_matched_offsets));
                }
            }
        }

        let map_keys: Vec<MapKey> = keys.iter().map(|&k| MapKey(k)).collect();
        if let Some((desc, offsets)) = self.auto_shapes.get(&map_keys) {
            let desc = *desc;
            self.last_matched_keys = keys.to_vec();
            self.last_matched_descriptor = Some(desc);
            self.last_matched_offsets = offsets.clone();
            return Some((desc, &self.last_matched_offsets));
        }

        let mut field_indices = FnvHashMap::default();
        let mut offsets = Vec::with_capacity(keys.len());
        for (idx, &key) in keys.iter().enumerate() {
            field_indices.insert(MapKey(key), idx);
            offsets.push(idx);
        }
        let desc = crate::vm::shape::alloc_shape_descriptor(StructDescriptor::new(
            "Object",
            field_indices,
            FnvHashMap::default(),
        ));

        self.auto_shapes.insert(map_keys, (desc, offsets.clone()));
        self.last_matched_keys = keys.to_vec();
        self.last_matched_descriptor = Some(desc);
        self.last_matched_offsets = offsets;
        Some((desc, &self.last_matched_offsets))
    }
}

impl VM {
    #[inline]
    pub fn find_matching_struct(&self, keys: &[Value]) -> Option<ShapePtr> {
        self.shapes.find_matching_struct(keys)
    }

    #[inline]
    pub fn find_matching_struct_cached(&mut self, keys: &[Value]) -> Option<(ShapePtr, &[usize])> {
        self.shapes.find_matching_struct_cached(keys)
    }
}

