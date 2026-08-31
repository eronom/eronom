use std::collections::HashMap;
use crate::vm::value::Value;
use crate::vm::execute::VM;
use crate::vm::gc::GcData;

pub fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

pub struct MultipartPart {
    pub headers: HashMap<String, String>,
    pub data: Vec<u8>,
}

pub fn parse_header_params(header_val: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();
    for part in header_val.split(';') {
        let part = part.trim();
        if let Some(pos) = part.find('=') {
            let key = part[..pos].trim().to_lowercase();
            let mut val = part[pos + 1..].trim();
            if val.starts_with('"') && val.ends_with('"') && val.len() >= 2 {
                val = &val[1..val.len() - 1];
            }
            params.insert(key, val.to_string());
        }
    }
    params
}

pub fn parse_part_bytes(part_bytes: &[u8]) -> Option<MultipartPart> {
    let mut part = part_bytes;
    if part.starts_with(b"\r\n") {
        part = &part[2..];
    } else if part.starts_with(b"\n") {
        part = &part[1..];
    }
    
    let d_crlf = b"\r\n\r\n";
    if let Some(headers_end) = find_subslice(part, d_crlf) {
        let headers_part = &part[..headers_end];
        let mut data_part = &part[headers_end + 4..];
        
        if data_part.ends_with(b"\r\n") {
            data_part = &data_part[..data_part.len() - 2];
        } else if data_part.ends_with(b"\n") {
            data_part = &data_part[..data_part.len() - 1];
        }
        
        if headers_part.starts_with(b"--") {
            return None;
        }
        
        let mut headers = HashMap::new();
        let headers_str = std::str::from_utf8(headers_part).unwrap_or("");
        for line in headers_str.lines() {
            if let Some(pos) = line.find(": ") {
                let key = line[..pos].to_lowercase();
                let val = &line[pos + 2..];
                headers.insert(key, val.to_string());
            }
        }
        
        return Some(MultipartPart {
            headers,
            data: data_part.to_vec(),
        });
    }
    None
}

pub fn parse_multipart(body: &[u8], boundary: &str) -> Vec<MultipartPart> {
    let boundary_marker = format!("--{}", boundary).into_bytes();
    let mut parts = Vec::new();
    let mut current_pos = 0;

    while current_pos < body.len() {
        let remaining = &body[current_pos..];
        if let Some(found_idx) = find_subslice(remaining, &boundary_marker) {
            let start_of_part = current_pos + found_idx + boundary_marker.len();
            let next_remaining = &body[start_of_part..];
            if let Some(end_idx) = find_subslice(next_remaining, &boundary_marker) {
                let part_bytes = &body[start_of_part..start_of_part + end_idx];
                if let Some(part) = parse_part_bytes(part_bytes) {
                    parts.push(part);
                }
                current_pos = start_of_part + end_idx;
            } else {
                let part_bytes = next_remaining;
                if let Some(part) = parse_part_bytes(part_bytes) {
                    parts.push(part);
                }
                break;
            }
        } else {
            break;
        }
    }
    parts
}

pub fn construct_file_object(vm: &mut VM, name: &str, type_str: &str, size: usize) -> Value {
    let name_val = Value::string(crate::vm::gc::get_or_create_string(name));
    let type_val = Value::string(crate::vm::gc::get_or_create_string(type_str));
    let size_val = Value::number(size as f64);

    if let Some(desc) = vm.structs.get("File") {
        let count = desc.field_indices.len();
        let mut fields = crate::vm::gc::get_pooled_vec(count);
        fields.resize(count, Value::null());

        let name_key = crate::vm::gc::get_or_create_string("name");
        let type_key = crate::vm::gc::get_or_create_string("type");
        let size_key = crate::vm::gc::get_or_create_string("size");

        if let Some(&idx) = desc.field_indices.get(&crate::vm::value::MapKey(Value::string(name_key))) {
            fields[idx] = name_val;
        }
        if let Some(&idx) = desc.field_indices.get(&crate::vm::value::MapKey(Value::string(type_key))) {
            fields[idx] = type_val;
        }
        if let Some(&idx) = desc.field_indices.get(&crate::vm::value::MapKey(Value::string(size_key))) {
            fields[idx] = size_val;
        }

        Value::object(crate::vm::gc::gc_allocate(GcData::Struct(crate::vm::gc::GcStruct {
            descriptor: desc.clone(),
            fields,
        })))
    } else {
        let mut map = crate::vm::gc::get_pooled_map(4);
        let name_key = crate::vm::gc::get_or_create_string("name");
        let type_key = crate::vm::gc::get_or_create_string("type");
        let size_key = crate::vm::gc::get_or_create_string("size");
        let is_file_key = crate::vm::gc::get_or_create_string("_isFile");

        map.insert(crate::vm::value::MapKey(Value::string(name_key)), name_val);
        map.insert(crate::vm::value::MapKey(Value::string(type_key)), type_val);
        map.insert(crate::vm::value::MapKey(Value::string(size_key)), size_val);
        map.insert(crate::vm::value::MapKey(Value::string(is_file_key)), Value::boolean(true));

        Value::object(crate::vm::gc::gc_allocate(GcData::Object(map)))
    }
}
