use crate::vm::value::{Value, MapKey, PTR_MASK, TAG_METHOD_JSON, TAG_METHOD_TEXT, TAG_METHOD_FILE};
use crate::vm::gc::{
    gc_allocate, gc_write_barrier, get_pooled_vec, get_pooled_map,
    get_or_create_string, gc_alloc_builtin_method, GcData, GcStruct, GcBoundMethod
};
use crate::vm::execute::builtins::{get_object_builtin_method_id, get_array_builtin_method_id, get_string_builtin_method_id};
use super::types::VM;

impl VM {
    pub unsafe fn execute_make_object(
        &mut self,
        dest: usize,
        start_reg: usize,
        count: usize,
        frame_slots: *mut Value,
    ) -> Result<(), String> {
        let ptr = if count == 0 {
            let obj = get_pooled_map(0);
            gc_allocate(GcData::Object(obj))
        } else if count <= 16 {
            let mut keys = [Value::null(); 16];
            let mut values = [Value::null(); 16];
            for i in 0..count {
                let key_val = *frame_slots.add(start_reg + i * 2);
                let val = *frame_slots.add(start_reg + i * 2 + 1);
                if !key_val.is_string() {
                    return Err("Object key must be string".into());
                }
                keys[i] = key_val;
                values[i] = val;
            }

            if let Some((desc, offsets)) = self.find_matching_struct_cached(&keys[..count]) {
                let mut fields = get_pooled_vec(count);
                fields.resize(count, Value::null());
                for i in 0..count {
                    let val = values[i];
                    let idx = offsets[i];
                    fields[idx] = val;
                }
                gc_allocate(GcData::Struct(GcStruct {
                    descriptor: desc,
                    fields,
                }))
            } else {
                let (desc, offsets) = crate::vm::shape::get_or_create_anonymous_shape(&keys[..count]);
                let mut fields = get_pooled_vec(count);
                fields.resize(count, Value::null());
                for i in 0..count {
                    fields[offsets[i]] = values[i];
                }
                gc_allocate(GcData::Struct(GcStruct {
                    descriptor: desc,
                    fields,
                }))
            }
        } else {
            let mut keys = Vec::with_capacity(count);
            let mut values = Vec::with_capacity(count);
            for i in 0..count {
                let key_val = *frame_slots.add(start_reg + i * 2);
                let val = *frame_slots.add(start_reg + i * 2 + 1);
                if !key_val.is_string() {
                    return Err("Object key must be string".into());
                }
                keys.push(key_val);
                values.push(val);
            }

            if let Some((desc, offsets)) = self.find_matching_struct_cached(&keys) {
                let mut fields = get_pooled_vec(keys.len());
                fields.resize(keys.len(), Value::null());
                for i in 0..count {
                    let val = values[i];
                    let idx = offsets[i];
                    fields[idx] = val;
                }
                gc_allocate(GcData::Struct(GcStruct {
                    descriptor: desc,
                    fields,
                }))
            } else {
                let (desc, offsets) = crate::vm::shape::get_or_create_anonymous_shape(&keys);
                let mut fields = get_pooled_vec(keys.len());
                fields.resize(keys.len(), Value::null());
                for i in 0..count {
                    fields[offsets[i]] = values[i];
                }
                gc_allocate(GcData::Struct(GcStruct {
                    descriptor: desc,
                    fields,
                }))
            }
        };
        *frame_slots.add(dest) = Value::object(ptr);
        Ok(())
    }

    pub unsafe fn execute_get_property(
        &mut self,
        dest: usize,
        obj: Value,
        name_val: Value,
        frame_slots: *mut Value,
    ) -> Result<(), String> {
        let name = name_val.as_str().unwrap_or("");
        if obj.is_object() {
            let ptr = obj.as_gc_ptr();
            let mut is_json_method = false;
            let mut is_text_method = false;
            if name == "json" || name == "text" {
                let body_key = get_or_create_string("_body");
                let is_response = match &(*ptr).data {
                    GcData::Object(map) => map.contains_key(&MapKey(Value::string(body_key))),
                    GcData::Struct(s) => s.get_field_by_name("_body").is_some(),
                    _ => false,
                };
                if is_response {
                    if name == "json" {
                        is_json_method = true;
                    } else {
                        is_text_method = true;
                    }
                }
            }
            let mut is_file_method = false;
            let mut file_method_sub_tag = 0;
            if name == "exists" || name == "text" || name == "json" {
                let is_file = match &(*ptr).data {
                    GcData::Object(map) => {
                        let file_key = get_or_create_string("_isFile");
                        map.get(&MapKey(Value::string(file_key)))
                            .map(|v| v.as_boolean())
                            .unwrap_or(false)
                    }
                    GcData::Struct(s) => {
                        s.descriptor.name.as_ref() == "File"
                    }
                    _ => false,
                };
                if is_file {
                    is_file_method = true;
                    file_method_sub_tag = match name {
                        "exists" => 0,
                        "text" => 1,
                        "json" => 2,
                        _ => 3,
                    };
                }
            }
            if is_json_method {
                *frame_slots.add(dest) = Value(TAG_METHOD_JSON | (ptr as u64 & PTR_MASK));
            } else if is_text_method {
                *frame_slots.add(dest) = Value(TAG_METHOD_TEXT | (ptr as u64 & PTR_MASK));
            } else if is_file_method && file_method_sub_tag < 3 {
                *frame_slots.add(dest) = Value(TAG_METHOD_FILE | (ptr as u64 & PTR_MASK & !3) | file_method_sub_tag);
            } else {
                match &(*ptr).data {
                    GcData::Object(map) => {
                        if let Some(val) = map.get(&MapKey(name_val)) {
                            *frame_slots.add(dest) = *val;
                        } else if let Some(m) = get_object_builtin_method_id(name) {
                            let ptr = gc_alloc_builtin_method(obj, m);
                            *frame_slots.add(dest) = Value::function(ptr);
                        } else {
                            *frame_slots.add(dest) = Value::null();
                        }
                    }
                    GcData::Struct(s) => {
                        if let Some(val) = s.get_field(name_val) {
                            *frame_slots.add(dest) = val;
                        } else if let Some(&method_val) = s.descriptor.methods.get(&MapKey(name_val)) {
                            let bound_method = GcBoundMethod {
                                receiver: obj,
                                function: method_val.as_gc_ptr(),
                            };
                            let ptr = gc_allocate(GcData::BoundMethod(bound_method));
                            *frame_slots.add(dest) = Value::function(ptr);
                        } else if let Some(m) = get_object_builtin_method_id(name) {
                            let ptr = gc_alloc_builtin_method(obj, m);
                            *frame_slots.add(dest) = Value::function(ptr);
                        } else {
                            *frame_slots.add(dest) = Value::null();
                        }
                    }
                    _ => {
                        *frame_slots.add(dest) = Value::null();
                    }
                }
            }
        } else if obj.is_array() {
            let ptr = obj.as_gc_ptr();
            match &(*ptr).data {
                GcData::Array(arr) => {
                    if name == "push" {
                        *frame_slots.add(dest) = Value::array_method_push(ptr);
                    } else if name == "pop" {
                        *frame_slots.add(dest) = Value::array_method_pop(ptr);
                    } else if name == "length" {
                        *frame_slots.add(dest) = Value::number(arr.len() as f64);
                    } else if let Some(m) = get_array_builtin_method_id(name) {
                        let ptr = gc_alloc_builtin_method(obj, m);
                        *frame_slots.add(dest) = Value::function(ptr);
                    } else if let Ok(idx) = name.parse::<usize>() {
                        let val = arr.get(idx).cloned().unwrap_or(Value::null());
                        *frame_slots.add(dest) = val;
                    } else {
                        *frame_slots.add(dest) = Value::null();
                    }
                }
                _ => {
                    *frame_slots.add(dest) = Value::null();
                }
            }
        } else if obj.is_string() {
            if name == "length" {
                let s = obj.as_str().unwrap_or("");
                *frame_slots.add(dest) = Value::number(s.chars().count() as f64);
            } else if let Some(m) = get_string_builtin_method_id(name) {
                let ptr = gc_alloc_builtin_method(obj, m);
                *frame_slots.add(dest) = Value::function(ptr);
            } else {
                *frame_slots.add(dest) = Value::null();
            }
        } else {
            return Err("Only objects, arrays, and strings have properties".into());
        }
        Ok(())
    }

    pub unsafe fn execute_set_property(
        &mut self,
        obj: Value,
        val: Value,
        name_val: Value,
    ) -> Result<(), String> {
        if obj.is_object() {
            let ptr = obj.as_gc_ptr();
            match &mut (*ptr).data {
                GcData::Object(map) => {
                    map.insert(MapKey(name_val), val);
                    gc_write_barrier(ptr, &val);
                }
                GcData::Struct(s) => {
                    if s.set_field(name_val, val) {
                        gc_write_barrier(ptr, &val);
                    } else if s.descriptor.name.as_ref() == "Anonymous" {
                        let new_desc = crate::vm::shape::transition_shape_add_property(&s.descriptor, name_val);
                        s.descriptor = new_desc;
                        s.fields.push(val);
                        gc_write_barrier(ptr, &val);
                    } else {
                        let name = name_val.as_str().unwrap_or("");
                        return Err(format!("Struct has no field '{}'", name));
                    }
                }
                _ => {}
            }
        } else if obj.is_array() {
            let name_rc = name_val.as_str().unwrap_or("");
            let ptr = obj.as_gc_ptr();
            match &mut (*ptr).data {
                GcData::Array(arr) => {
                    if let Ok(idx) = name_rc.parse::<usize>() {
                        if idx < arr.len() {
                            arr[idx] = val;
                        } else if idx == arr.len() {
                            arr.push(val);
                        } else {
                            return Err(format!(
                                "Index {} out of bounds for array of length {}",
                                idx,
                                arr.len()
                            ));
                        }
                        gc_write_barrier(ptr, &val);
                    } else {
                        return Err("Cannot set non-numeric property on array".into());
                    }
                }
                _ => {}
            }
        } else {
            return Err("Only objects and arrays have properties".into());
        }
        Ok(())
    }

    pub unsafe fn execute_get_index(
        &mut self,
        dest: usize,
        obj: Value,
        index: Value,
        frame_slots: *mut Value,
    ) -> Result<(), String> {
        if obj.is_array() {
            let ptr = obj.as_gc_ptr();
            if index.is_number() {
                let idx = index.as_number() as usize;
                match &(*ptr).data {
                    GcData::Array(arr) => {
                        let val = arr.get(idx).cloned().unwrap_or(Value::null());
                        *frame_slots.add(dest) = val;
                    }
                    _ => {
                        *frame_slots.add(dest) = Value::null();
                    }
                }
            } else if index.is_string() {
                let s = index.as_str().unwrap_or("");
                if let Ok(idx) = s.parse::<usize>() {
                    match &(*ptr).data {
                        GcData::Array(arr) => {
                            let val = arr.get(idx).cloned().unwrap_or(Value::null());
                            *frame_slots.add(dest) = val;
                        }
                        _ => {
                            *frame_slots.add(dest) = Value::null();
                        }
                    }
                } else {
                    *frame_slots.add(dest) = Value::null();
                }
            } else {
                return Err("Only arrays can be indexed by numbers, and objects by strings".into());
            }
        } else if obj.is_object() {
            let ptr = obj.as_gc_ptr();
            if index.is_string() {
                match &(*ptr).data {
                    GcData::Object(map) => {
                        let val = map.get(&MapKey(index)).cloned().unwrap_or(Value::null());
                        *frame_slots.add(dest) = val;
                    }
                    GcData::Struct(s) => {
                        let val = s.get_field(index).unwrap_or(Value::null());
                        *frame_slots.add(dest) = val;
                    }
                    _ => {
                        *frame_slots.add(dest) = Value::null();
                    }
                }
            } else {
                return Err("Only arrays can be indexed by numbers, and objects by strings".into());
            }
        } else {
            return Err("Only arrays can be indexed by numbers, and objects by strings".into());
        }
        Ok(())
    }

    pub unsafe fn execute_set_index(
        &mut self,
        obj: Value,
        index: Value,
        val: Value,
    ) -> Result<(), String> {
        if obj.is_array() {
            let ptr = obj.as_gc_ptr();
            if index.is_number() {
                let idx = index.as_number() as usize;
                match &mut (*ptr).data {
                    GcData::Array(arr) => {
                        if idx < arr.len() {
                            arr[idx] = val;
                        } else if idx == arr.len() {
                            arr.push(val);
                        } else {
                            return Err(format!(
                                "Index {} out of bounds for array of length {}",
                                idx,
                                arr.len()
                            ));
                        }
                        gc_write_barrier(ptr, &val);
                    }
                    _ => {}
                }
            } else if index.is_string() {
                let s = index.as_str().unwrap_or("");
                if let Ok(idx) = s.parse::<usize>() {
                    match &mut (*ptr).data {
                        GcData::Array(arr) => {
                            if idx < arr.len() {
                                arr[idx] = val;
                            } else if idx == arr.len() {
                                arr.push(val);
                            } else {
                                return Err(format!(
                                    "Index {} out of bounds for array of length {}",
                                    idx,
                                    arr.len()
                                ));
                            }
                            gc_write_barrier(ptr, &val);
                        }
                        _ => {}
                    }
                } else {
                    return Err("Cannot set non-numeric property on array".into());
                }
            } else {
                return Err("Only arrays can be indexed by numbers, and objects by strings".into());
            }
        } else if obj.is_object() {
            let ptr = obj.as_gc_ptr();
            if index.is_string() {
                match &mut (*ptr).data {
                    GcData::Object(map) => {
                        map.insert(MapKey(index), val);
                        gc_write_barrier(ptr, &val);
                    }
                    GcData::Struct(s) => {
                        if s.set_field(index, val) {
                            gc_write_barrier(ptr, &val);
                        } else if s.descriptor.name.as_ref() == "Anonymous" {
                            let new_desc = crate::vm::shape::transition_shape_add_property(&s.descriptor, index);
                            s.descriptor = new_desc;
                            s.fields.push(val);
                            gc_write_barrier(ptr, &val);
                        } else {
                            let name = index.as_str().unwrap_or("");
                            return Err(format!("Struct has no field '{}'", name));
                        }
                    }
                    _ => {}
                }
            } else {
                return Err("Only arrays can be indexed by numbers, and objects by strings".into());
            }
        } else {
            return Err("Only arrays can be indexed by numbers, and objects by strings".into());
        }
        Ok(())
    }
}
