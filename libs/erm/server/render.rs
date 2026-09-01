use std::ffi::{c_char, c_void, CString};
use std::fs;
use std::path::Path;
use crate::compiler;
use crate::vm::value::Value;
use super::types::*;

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
    let ptr = crate::vm::gc::get_or_create_string(&time_str);
    Value::string(ptr)
}

pub fn value_to_string_for_render(v: Value) -> String {
    if let Some(s) = v.as_str() {
        s.to_string()
    } else if v.is_null() {
        "null".to_string()
    } else if v.is_boolean() {
        v.as_boolean().to_string()
    } else if v.is_number() {
        v.as_number().to_string()
    } else {
        let json_val = crate::vm::er_http::value_to_json(v);
        serde_json::to_string(&json_val).unwrap_or_else(|_| "null".to_string())
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
                crate::vm::gc::GcData::Object(map) => {
                    for (k, v) in map {
                        if let Some(key_str) = k.0.as_str() {
                            let val_str = value_to_string_for_render(*v);
                            params_map.insert(key_str.to_string(), val_str);
                        }
                    }
                }
                crate::vm::gc::GcData::Struct(s) => {
                    for (map_key, &idx) in &s.descriptor.field_indices {
                        if let Some(key_str) = map_key.0.as_str() {
                            let val_str = value_to_string_for_render(s.fields[idx]);
                            params_map.insert(key_str.to_string(), val_str);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    
    let base_path = BASE_PATH.lock().unwrap().clone().unwrap_or_else(|| std::path::PathBuf::from("."));
    let mut path = base_path.join(file_path);
    if !path.exists() {
        if let Some(folder_name) = base_path.file_name().and_then(|n| n.to_str()) {
            let fp = Path::new(file_path);
            if fp.starts_with(folder_name) {
                if let Ok(stripped) = fp.strip_prefix(folder_name) {
                    path = base_path.join(stripped);
                }
            }
        }
    }
    if !path.exists() {
        return Value::null();
    }
    
    let is_prod = *IS_PROD.lock().unwrap();
    let is_html = path.extension().map_or(false, |ext| ext == "html");

    if is_html {
        // PPR/SSG Path: Bypass compiler entirely
        let content = fs::read_to_string(&path).unwrap_or_default();

        // Construct parameters string: window.__erm_params = {...};
        let mut params_js = String::from("window.__erm_params = {");
        for (i, (k, v)) in params_map.iter().enumerate() {
            if i > 0 {
                params_js.push_str(", ");
            }
            let is_numeric = v.parse::<f64>().is_ok();
            let is_boolean = v == "true" || v == "false";
            if is_numeric || is_boolean {
                params_js.push_str(&format!("\"{}\": {}", k, v));
            } else {
                params_js.push_str(&format!("\"{}\": \"{}\"", k, v.replace('"', "\\\"")));
            }
        }
        params_js.push_str("};");

        let processed = content.replace("window.__erm_params = {};", &params_js);
        let ptr = crate::vm::gc::get_or_create_string(&processed);
        Value::string(ptr)
    } else {
        // SSR Path (.erm files)
        match fs::read_to_string(&path) {
            Ok(content) => {
                let parent = path.parent().unwrap().to_string_lossy();
                match compiler::process_erm_component(path.to_str().unwrap_or(&parent), &content, is_prod, &params_map) {
                    Ok(html) => {
                        let ptr = crate::vm::gc::get_or_create_string(&html);
                        Value::string(ptr)
                    }
                    Err(e) => {
                        eprintln!("[render] Compiler error: {:?}", e);
                        Value::null()
                    }
                }
            }
            Err(_) => Value::null(),
        }
    }
}

pub fn execute_api_route(
    res: *mut c_void,
    method: &str,
    target: &str,
    headers: &str,
    body: &[u8],
    file_path: &Path,
) -> anyhow::Result<()> {
    let base_path = BASE_PATH.lock().unwrap().clone().unwrap();
    let rel = file_path.strip_prefix(&base_path)?;
    let parent = rel.parent().unwrap_or(Path::new(""));
    let mut prefix = if parent.as_os_str().is_empty() {
        String::new()
    } else {
        format!("/{}", parent.to_string_lossy().replace('\\', "/"))
    };
    if prefix.starts_with("/server/") {
        prefix = prefix["/server".len()..].to_string();
    }
    if prefix.contains("/pages") || prefix.starts_with("/pages") || prefix.ends_with("/pages") {
        prefix = String::new();
    }

    let stmts = match crate::frontend::parse_and_resolve_imports(file_path) {
        Ok(s) => s,
        Err(e) => anyhow::bail!("Compile/Import error: {}", e),
    };

    let compiler = crate::vm::compiler::Compiler::new();
    let function = match compiler.compile(&stmts) {
        Ok(f) => f,
        Err(e) => anyhow::bail!("Compile error: {}", e),
    };

    crate::vm::er_http::ROUTER.with(|r| r.borrow_mut().clear());
    crate::vm::er_http::ROUTES.with(|r| r.borrow_mut().clear());
    crate::vm::er_http::WS_ROUTES.with(|w| w.borrow_mut().clear());
    crate::vm::er_http::MIDDLEWARES.with(|m| m.borrow_mut().clear());
    crate::vm::er_http::STATIC_MOUNTS.with(|s| s.borrow_mut().clear());
    
    crate::vm::er_http::ROUTE_PREFIX.with(|p| {
        *p.borrow_mut() = Some(prefix);
    });

    let mut vm = crate::vm::execute::VM::new();
    vm.register_global("print", Value::native_function(native_print));
    vm.register_global("router", Value::native_function(crate::vm::er_http::native_route));
    vm.register_global("render", Value::native_function(native_render));
    vm.register_global("fetch", Value::native_function(crate::vm::er_http::native_fetch));
    vm.register_global("setTimeout", Value::native_function(crate::vm::er_http::native_set_timeout));
    vm.register_global("fetchSync", Value::native_function(crate::vm::er_http::native_fetch_sync));
    vm.register_global("fetchEvented", Value::native_function(crate::vm::er_http::native_fetch_evented));
    vm.register_global("futureAwait", Value::native_function(crate::vm::er_http::native_future_await));
    vm.register_global("arrayLen", Value::native_function(crate::vm::er_http::native_array_len));
    vm.register_global("sleep", Value::native_function(crate::vm::er_http::native_sleep));
    vm.register_global("createPromisePair", Value::native_function(crate::vm::er_http::native_create_promise_pair));
    vm.register_global("setIoMode", Value::native_function(crate::vm::er_http::native_set_io_mode));
    vm.register_global("getIoMode", Value::native_function(crate::vm::er_http::native_get_io_mode));
    vm.register_global("now", Value::native_function(native_now));
    vm.register_global("localTimeString", Value::native_function(native_local_time_string));
    crate::vm::er_http::register_eronom_file_api(&mut vm).unwrap();
    crate::vm::std_fs::register_fs_natives(&mut vm);
    crate::vm::std_path::register_path_natives(&mut vm);
    crate::vm::std_crypto::register_crypto_natives(&mut vm);
    crate::vm::std_json::register_json_natives(&mut vm);
    crate::vm::std_system::register_system_natives(&mut vm);
    crate::vm::er_http::set_target_script_path(&file_path.to_string_lossy());

    let _guard = GcGuard;
    
    // Load config from eronom.toml if it exists
    if let Some(parent_dir) = file_path.parent() {
        let toml_path = parent_dir.join("eronom.toml");
        if toml_path.exists() {
            if let Ok(toml_content) = std::fs::read_to_string(&toml_path) {
                if let Ok(toml_val) = toml::from_str::<toml::Value>(&toml_content) {
                    if let Ok(json_val) = serde_json::to_value(toml_val) {
                        let config_val = crate::vm::gc::json_to_value(json_val);
                        vm.register_global("config", config_val);
                    }
                }
            }
        }
    }

    if let Err(e) = vm.run(function) {
        anyhow::bail!("VM Runtime error: {}", e);
    }
    if let Err(e) = vm.run_event_loop() {
        anyhow::bail!("VM Event loop error: {}", e);
    }

    let method_c = CString::new(method).unwrap();
    let target_c = CString::new(target).unwrap();
    
    crate::vm::er_http::ACTIVE_VM.with(|active| {
        active.set(&mut vm as *mut crate::vm::execute::VM);
    });

    let headers_c = CString::new(headers).unwrap();
    crate::vm::er_http::er_http_on_request(
        res,
        method_c.as_ptr(),
        method.len(),
        target_c.as_ptr(),
        target.len(),
        headers_c.as_ptr(),
        headers.len(),
        body.as_ptr() as *const c_char,
        body.len(),
    );

    crate::vm::er_http::ACTIVE_VM.with(|active| {
        active.set(std::ptr::null_mut());
    });

    crate::vm::er_http::ROUTE_PREFIX.with(|p| {
        *p.borrow_mut() = None;
    });

    Ok(())
}
