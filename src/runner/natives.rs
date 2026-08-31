use eronom::vm as backend;
use backend::Value;

#[repr(C)]
pub struct Tm {
    pub tm_sec: std::ffi::c_int,
    pub tm_min: std::ffi::c_int,
    pub tm_hour: std::ffi::c_int,
    pub tm_mday: std::ffi::c_int,
    pub tm_mon: std::ffi::c_int,
    pub tm_year: std::ffi::c_int,
    pub tm_wday: std::ffi::c_int,
    pub tm_yday: std::ffi::c_int,
    pub tm_isdst: std::ffi::c_int,
    #[cfg(unix)]
    pub tm_gmtoff: std::ffi::c_long,
    #[cfg(unix)]
    pub tm_zone: *const std::ffi::c_char,
}

#[cfg(unix)]
unsafe extern "C" {
    pub fn time(time: *mut std::ffi::c_long) -> std::ffi::c_long;
    pub fn localtime_r(timep: *const std::ffi::c_long, result: *mut Tm) -> *mut Tm;
}

#[cfg(windows)]
unsafe extern "C" {
    pub fn _time64(time: *mut i64) -> i64;
    pub fn _localtime64_s(result: *mut Tm, timep: *const i64) -> std::ffi::c_int;
}

pub fn get_local_time_string() -> String {
    unsafe {
        let mut tm_val = std::mem::zeroed::<Tm>();
        #[cfg(unix)]
        {
            let mut t: std::ffi::c_long = 0;
            time(&mut t);
            localtime_r(&t, &mut tm_val);
        }
        #[cfg(windows)]
        {
            let mut t: i64 = 0;
            _time64(&mut t);
            _localtime64_s(&mut tm_val, &t);
        }
        let hour = tm_val.tm_hour;
        let min = tm_val.tm_min;
        let sec = tm_val.tm_sec;
        let am_pm = if hour >= 12 { "PM" } else { "AM" };
        let display_hour = if hour == 0 {
            12
        } else if hour > 12 {
            hour - 12
        } else {
            hour
        };
        format!("{:02}:{:02}:{:02} {}", display_hour, min, sec, am_pm)
    }
}

pub fn native_print(args: Vec<Value>) -> Value {
    let mut outputs = Vec::new();
    for arg in args {
        outputs.push(arg.to_string());
    }
    println!("{}", outputs.join(" "));
    Value::null()
}

pub fn native_now(_args: Vec<Value>) -> Value {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    Value::number(now as f64)
}

pub fn native_local_time_string(_args: Vec<Value>) -> Value {
    let time_str = get_local_time_string();
    let ptr = backend::gc::gc_alloc_string(&time_str);
    Value::string(ptr)
}

pub fn value_to_json(val: Value) -> String {
    if val.is_null() {
        "null".to_string()
    } else if val.is_boolean() {
        val.as_boolean().to_string()
    } else if val.is_number() {
        val.as_number().to_string()
    } else if val.is_string() {
        format!("\"{}\"", val.as_str().unwrap_or("").replace("\"", "\\\""))
    } else if val.is_array() {
        unsafe {
            match &(*val.as_gc_ptr()).data {
                backend::GcData::Array(arr) => {
                    let items: Vec<String> = arr.iter().map(|&v| value_to_json(v)).collect();
                    format!("[{}]", items.join(","))
                }
                _ => "[]".to_string(),
            }
        }
    } else if val.is_object() {
        unsafe {
            match &(*val.as_gc_ptr()).data {
                backend::GcData::Object(obj) => {
                    let mut items = Vec::new();
                    for (k, &v) in obj {
                        let s = k.0.as_str().unwrap_or("");
                        items.push(format!("\"{}\":{}", s, value_to_json(v)));
                    }
                    format!("{{{}}}", items.join(","))
                }
                backend::GcData::Struct(s) => {
                    let mut items = Vec::new();
                    for (map_key, &idx) in &s.descriptor.field_indices {
                        let name = map_key.0.as_str().unwrap_or("");
                        items.push(format!("\"{}\":{}", name, value_to_json(s.fields[idx])));
                    }
                    format!("{{{}}}", items.join(","))
                }
                _ => "{}".to_string(),
            }
        }
    } else {
        "null".to_string()
    }
}

pub fn native_render(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::null();
    }
    let file_path_val = args[0];
    let params_val = args[1];
    
    let file_path = match file_path_val.as_str() {
        Some(s) => s,
        None => return Value::null(),
    };
    
    let mut params_map = std::collections::HashMap::new();
    if params_val.is_object() {
        unsafe {
            match &(*params_val.as_gc_ptr()).data {
                backend::GcData::Object(map) => {
                    for (k, v) in map {
                        if let Some(key_str) = k.0.as_str() {
                            let val_str = if v.is_string() {
                                v.as_str().unwrap_or("").to_string()
                            } else if v.is_array() || v.is_object() {
                                value_to_json(*v)
                            } else {
                                v.to_string()
                            };
                            params_map.insert(key_str.to_string(), val_str);
                        }
                    }
                }
                backend::GcData::Struct(s) => {
                    for (map_key, &idx) in &s.descriptor.field_indices {
                        let name = map_key.0.as_str().unwrap_or("");
                        let v = s.fields[idx];
                        let val_str = if v.is_string() {
                            v.as_str().unwrap_or("").to_string()
                        } else if v.is_array() || v.is_object() {
                            value_to_json(v)
                        } else {
                            v.to_string()
                        };
                        params_map.insert(name.to_string(), val_str);
                    }
                }
                _ => {}
            }
        }
    }
    
    let path = std::path::Path::new(file_path);
    let mut resolved_path = if path.is_relative() {
        if let Some(script_path) = backend::er_http::get_target_script_path() {
            if let Some(parent) = std::path::Path::new(&script_path).parent() {
                parent.join(path)
            } else {
                path.to_path_buf()
            }
        } else {
            path.to_path_buf()
        }
    } else {
        path.to_path_buf()
    };
    
    if !resolved_path.exists() {
        if let Some(first_part) = path.iter().next().and_then(|p| p.to_str()) {
            if let Ok(stripped) = path.strip_prefix(first_part) {
                let fallback = if stripped.is_relative() {
                    if let Some(script_path) = backend::er_http::get_target_script_path() {
                        if let Some(parent) = std::path::Path::new(&script_path).parent() {
                            parent.join(stripped)
                        } else {
                            stripped.to_path_buf()
                        }
                    } else {
                        stripped.to_path_buf()
                    }
                } else {
                    stripped.to_path_buf()
                };
                if fallback.exists() {
                    resolved_path = fallback;
                }
            }
        }
    }
    
    let base_dir = match resolved_path.parent() {
        Some(p) => p.to_string_lossy().to_string(),
        None => "".to_string(),
    };

    let (content, is_html, comp_path) = if resolved_path.exists() {
        let c = match std::fs::read_to_string(&resolved_path) {
            Ok(c) => c,
            Err(_) => return Value::null(),
        };
        let is_h = resolved_path.extension().map_or(false, |ext| ext == "html");
        let cp = resolved_path.to_str().unwrap_or(&base_dir).to_string();
        (c, is_h, cp)
    } else if let Some(vfs_text) = backend::embedded::get_vfs_text(file_path) {
        let is_h = file_path.ends_with(".html");
        (vfs_text, is_h, file_path.to_string())
    } else if let Some(vfs_text) = backend::embedded::get_vfs_text(&resolved_path.to_string_lossy()) {
        let is_h = resolved_path.extension().map_or(false, |ext| ext == "html");
        (vfs_text, is_h, resolved_path.to_string_lossy().to_string())
    } else {
        return Value::null();
    };

    if is_html {
        let mut final_content = content;
        if !params_map.is_empty() {
            let mut params_js = String::from("window.__erm_params = {");
            for (k, v) in &params_map {
                params_js.push_str(&format!("{}: \"{}\",", k, v.replace("\"", "\\\"")));
            }
            params_js.push_str("};");
            final_content = final_content.replace("window.__erm_params = {};", &params_js);
        }
        let ptr = backend::gc::gc_alloc_string(&final_content);
        return Value::string(ptr);
    }
    
    match eronom::compiler::process_erm_component(&comp_path, &content, true, &params_map) {
        Ok(html) => {
            let ptr = backend::gc::gc_alloc_string(&html);
            Value::string(ptr)
        }
        Err(e) => {
            eprintln!("[render] Compiler error: {:?}", e);
            Value::null()
        }
    }
}
