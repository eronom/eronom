use std::path::{Path, PathBuf};
use std::fs;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, SystemTime};
use std::collections::HashMap;
use std::ffi::{c_char, c_void, CString};
use crate::compiler;

static BASE_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
static DEFAULT_FILE: Mutex<Option<PathBuf>> = Mutex::new(None);
static IS_PROD: Mutex<bool> = Mutex::new(false);

use std::sync::LazyLock;

static HTML_SHELL_CACHE: LazyLock<Mutex<HashMap<PathBuf, String>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static SSR_CACHE: LazyLock<Mutex<HashMap<(PathBuf, Vec<(String, String)>), String>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn get_sorted_params(map: &HashMap<String, String>) -> Vec<(String, String)> {
    let mut vec: Vec<(String, String)> = map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    vec.sort_by(|a, b| a.0.cmp(&b.0));
    vec
}

static HMR_QUEUE: Mutex<Vec<String>> = Mutex::new(Vec::new());
static ACTIVE_CONNECTIONS: Mutex<Vec<usize>> = Mutex::new(Vec::new());

unsafe extern "C" {
    fn er_http_init_with_callbacks(
        http_req_cb: extern "C" fn(
            *mut c_void,
            *const c_char,
            usize,
            *const c_char,
            usize,
            *const c_char,
            usize,
            *const c_char,
            usize,
        ),
        ws_open_cb: extern "C" fn(*mut c_void, *const c_char, usize),
        ws_message_cb: extern "C" fn(*mut c_void, *const c_char, usize, *const c_char, usize),
        ws_close_cb: extern "C" fn(*mut c_void, *const c_char, usize, i32, *const c_char, usize),
    );
    fn er_ws_register_route(path: *const c_char);
    fn er_ws_send(ws: *mut c_void, message: *const c_char, message_len: usize);
    fn er_http_listen_and_run(port: i32);
    
    fn er_http_response_write_status(res: *mut c_void, status_str: *const c_char, status_len: usize);
    fn er_http_response_write_header(res: *mut c_void, key_str: *const c_char, key_len: usize, val_str: *const c_char, val_len: usize);
    fn er_http_response_end(res: *mut c_void, data_str: *const c_char, data_len: usize);
    
    fn er_http_create_timer(ms: i32, cb: extern "C" fn(*mut c_void));
}

fn scan_directory(dir: &Path, files: &mut HashMap<PathBuf, SystemTime>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            if name_str.starts_with('.') ||
               name_str == "target" ||
               name_str == "build" ||
               name_str == "node_modules" {
                continue;
            }

            if path.is_dir() {
                scan_directory(&path, files);
            } else if path.is_file() {
                if let Some(ext) = path.extension() {
                    let ext_str = ext.to_string_lossy();
                    if ext_str == "erm" || ext_str == "css" || ext_str == "js" {
                        if let Ok(metadata) = entry.metadata() {
                            if let Ok(modified) = metadata.modified() {
                                files.insert(path, modified);
                            }
                        }
                    }
                }
            }
        }
    }
}

extern "C" fn dev_http_callback(
    res: *mut c_void,
    method_ptr: *const c_char,
    method_len: usize,
    path_ptr: *const c_char,
    path_len: usize,
    headers_ptr: *const c_char,
    headers_len: usize,
    body_ptr: *const c_char,
    body_len: usize,
) {
    let method = unsafe {
        let slice = std::slice::from_raw_parts(method_ptr as *const u8, method_len);
        std::str::from_utf8(slice).unwrap_or("")
    };
    let path = unsafe {
        let slice = std::slice::from_raw_parts(path_ptr as *const u8, path_len);
        std::str::from_utf8(slice).unwrap_or("")
    };
    let headers = unsafe {
        let slice = std::slice::from_raw_parts(headers_ptr as *const u8, headers_len);
        std::str::from_utf8(slice).unwrap_or("")
    };
    let body = unsafe {
        if body_ptr.is_null() {
            &[]
        } else {
            std::slice::from_raw_parts(body_ptr as *const u8, body_len)
        }
    };

    if let Err(e) = handle_dev_request(res, method, path, headers, body) {
        let err_msg = format!("Internal Server Error: {}", e);
        let err_bytes = err_msg.as_bytes();
        unsafe {
            let status = CString::new("500 Internal Server Error").unwrap();
            er_http_response_write_status(res, status.as_ptr(), status.as_bytes().len());
            let content_type = CString::new("text/plain; charset=utf-8").unwrap();
            let content_type_key = CString::new("Content-Type").unwrap();
            er_http_response_write_header(res, content_type_key.as_ptr(), content_type_key.as_bytes().len(), content_type.as_ptr(), content_type.as_bytes().len());
            er_http_response_end(res, err_msg.as_ptr() as *const c_char, err_bytes.len());
        }
    }
}

extern "C" fn dev_ws_open_callback(
    ws: *mut c_void,
    _path_ptr: *const c_char,
    _path_len: usize,
) {
    let mut conns = ACTIVE_CONNECTIONS.lock().unwrap();
    if !conns.contains(&(ws as usize)) {
        conns.push(ws as usize);
    }
}

extern "C" fn dev_ws_message_callback(
    _ws: *mut c_void,
    _path_ptr: *const c_char,
    _path_len: usize,
    _msg_ptr: *const c_char,
    _msg_len: usize,
) {
}

extern "C" fn dev_ws_close_callback(
    ws: *mut c_void,
    _path_ptr: *const c_char,
    _path_len: usize,
    _code: i32,
    _msg_ptr: *const c_char,
    _msg_len: usize,
) {
    let mut conns = ACTIVE_CONNECTIONS.lock().unwrap();
    if let Some(pos) = conns.iter().position(|&x| x == ws as usize) {
        conns.remove(pos);
    }
}

extern "C" fn check_hmr_queue(_timer: *mut c_void) {
    let mut queue = HMR_QUEUE.lock().unwrap();
    if !queue.is_empty() {
        let conns = ACTIVE_CONNECTIONS.lock().unwrap();
        for &ws in conns.iter() {
            for msg in queue.iter() {
                let msg_c = CString::new(msg.as_str()).unwrap();
                unsafe {
                    er_ws_send(ws as *mut c_void, msg_c.as_ptr(), msg_c.as_bytes().len());
                }
            }
        }
        queue.clear();
    }
}

use crate::vm::value::Value;

struct GcGuard;
impl Drop for GcGuard {
    fn drop(&mut self) {
        crate::vm::gc::gc_free_all();
    }
}

fn native_print(args: Vec<Value>) -> Value {
    let mut outputs = Vec::new();
    for arg in args {
        outputs.push(arg.to_string());
    }
    println!("{}", outputs.join(" "));
    Value::null()
}

#[repr(C)]
struct Tm {
    tm_sec: std::ffi::c_int,
    tm_min: std::ffi::c_int,
    tm_hour: std::ffi::c_int,
    tm_mday: std::ffi::c_int,
    tm_mon: std::ffi::c_int,
    tm_year: std::ffi::c_int,
    tm_wday: std::ffi::c_int,
    tm_yday: std::ffi::c_int,
    tm_isdst: std::ffi::c_int,
    tm_gmtoff: std::ffi::c_long,
    tm_zone: *const std::ffi::c_char,
}

unsafe extern "C" {
    fn time(time: *mut std::ffi::c_long) -> std::ffi::c_long;
    fn localtime_r(timep: *const std::ffi::c_long, result: *mut Tm) -> *mut Tm;
}

fn get_local_time_string() -> String {
    unsafe {
        let mut t: std::ffi::c_long = 0;
        time(&mut t);
        let mut tm_val = std::mem::zeroed::<Tm>();
        localtime_r(&t, &mut tm_val);
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

fn native_now(_args: Vec<Value>) -> Value {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    Value::number(now as f64)
}

fn native_local_time_string(_args: Vec<Value>) -> Value {
    let time_str = get_local_time_string();
    let ptr = crate::vm::gc::get_or_create_string(&time_str);
    Value::string(ptr)
}


fn value_to_string_for_render(v: Value) -> String {
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

fn native_render(args: Vec<Value>) -> Value {
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
        let content = if is_prod {
            let mut cache = HTML_SHELL_CACHE.lock().unwrap();
            if let Some(cached) = cache.get(&path) {
                cached.clone()
            } else {
                let loaded = fs::read_to_string(&path).unwrap_or_default();
                cache.insert(path.clone(), loaded.clone());
                loaded
            }
        } else {
            fs::read_to_string(&path).unwrap_or_default()
        };

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
        if is_prod {
            let sorted_params = get_sorted_params(&params_map);
            let cache_key = (path.clone(), sorted_params);
            {
                let cache = SSR_CACHE.lock().unwrap();
                if let Some(cached_html) = cache.get(&cache_key) {
                    let ptr = crate::vm::gc::get_or_create_string(cached_html);
                    return Value::string(ptr);
                }
            }
            
            // Not in cache, compile and cache
            match fs::read_to_string(&path) {
                Ok(content) => {
                    let parent = path.parent().unwrap().to_string_lossy();
                    match compiler::process_erm_component(path.to_str().unwrap_or(&parent), &content, is_prod, &params_map) {
                        Ok(html) => {
                            let mut cache = SSR_CACHE.lock().unwrap();
                            cache.insert(cache_key, html.clone());
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
        } else {
            // Dev SSR Path (always read & compile)
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
}

fn execute_api_route(
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

    crate::vm::er_http::ROUTES.with(|r| r.borrow_mut().clear());
    crate::vm::er_http::WS_ROUTES.with(|w| w.borrow_mut().clear());
    crate::vm::er_http::MIDDLEWARES.with(|m| m.borrow_mut().clear());
    
    crate::vm::er_http::ROUTE_PREFIX.with(|p| {
        *p.borrow_mut() = Some(prefix);
    });

    let mut vm = crate::vm::execute::VM::new();
    vm.register_global("print", Value::native_function(native_print));
    vm.register_global("router", Value::native_function(crate::vm::er_http::native_route));
    vm.register_global("render", Value::native_function(native_render));
    vm.register_global("fetch", Value::native_function(crate::vm::er_http::native_fetch));
    vm.register_global("setTimeout", Value::native_function(crate::vm::er_http::native_set_timeout));
    vm.register_global("fetchAsync", Value::native_function(crate::vm::er_http::native_fetch_async));
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
    crate::vm::er_http::set_target_script_path(&file_path.to_string_lossy());

    let _guard = GcGuard;
    
    // Load config.er if it exists
    if let Some(parent_dir) = file_path.parent() {
        let config_path = parent_dir.join("config.er");
        if config_path.exists() {
            if let Ok(config_content) = std::fs::read_to_string(&config_path) {
                let config_tokens = crate::frontend::lex(&config_content);
                let mut config_parser = crate::frontend::Parser::new(config_tokens);
                if let Ok(config_stmts) = config_parser.parse() {
                    let config_compiler = crate::vm::compiler::Compiler::new();
                    if let Ok(config_func) = config_compiler.compile(&config_stmts) {
                        let _ = vm.run(config_func);
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
    unsafe {
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
    }

    crate::vm::er_http::ACTIVE_VM.with(|active| {
        active.set(std::ptr::null_mut());
    });

    crate::vm::er_http::ROUTE_PREFIX.with(|p| {
        *p.borrow_mut() = None;
    });

    Ok(())
}

fn handle_dev_request(res: *mut c_void, method: &str, target: &str, headers: &str, body: &[u8]) -> anyhow::Result<()> {
    let base_path = BASE_PATH.lock().unwrap().clone().unwrap();
    let default_file = DEFAULT_FILE.lock().unwrap().clone();
    let is_prod = *IS_PROD.lock().unwrap();

    println!("Request: {} {}", method, target);

    if target.starts_with("/__erm_src/") {
        let rel_file = &target["/__erm_src/".len()..];
        let file_path = base_path.join(rel_file);
        if file_path.exists() && file_path.is_file() {
            if let Ok(src) = fs::read_to_string(&file_path) {
                unsafe {
                    let status = CString::new("200 OK").unwrap();
                    er_http_response_write_status(res, status.as_ptr(), status.as_bytes().len());
                    let content_type = CString::new("text/plain; charset=utf-8").unwrap();
                    let content_type_key = CString::new("Content-Type").unwrap();
                    er_http_response_write_header(res, content_type_key.as_ptr(), content_type_key.as_bytes().len(), content_type.as_ptr(), content_type.as_bytes().len());
                    er_http_response_end(res, src.as_ptr() as *const c_char, src.len());
                }
                return Ok(());
            }
        }
    }

    let app_dir = if base_path.join("app").exists() {
        base_path.join("app")
    } else {
        base_path.clone()
    };

    let mut params = HashMap::new();
    let file_path = if target == "/" {
        if let Some(ref def_file) = default_file {
            def_file.clone()
        } else {
            let index_erm = app_dir.join("pages").join("index.erm");
            if index_erm.exists() {
                index_erm
            } else {
                app_dir.join("pages").join("index.html")
            }
        }
    } else {
        let resolve_root = if !target.starts_with("/api/") {
            app_dir.join("pages")
        } else {
            base_path.join("server")
        };

        if let Some((path, p)) = resolve_path(&resolve_root, target) {
            params = p;
            path
        } else {
            let primary_fallback = resolve_root.join(&target[1..]);
            if !target.starts_with("/api/") && !primary_fallback.exists() {
                let secondary_fallback = app_dir.join(&target[1..]);
                if secondary_fallback.exists() {
                    secondary_fallback
                } else {
                    let base_fallback = base_path.join(&target[1..]);
                    if base_fallback.exists() {
                        base_fallback
                    } else {
                        primary_fallback
                    }
                }
            } else {
                primary_fallback
            }
        }
    };

    let mut file_path = file_path;
    if !file_path.exists() && default_file.is_some() {
        file_path = default_file.clone().unwrap();
    }

    if file_path.exists() && file_path.is_file() {
        if file_path.extension().map_or(false, |ext| ext == "erm") {
            let cached_html = if is_prod {
                let sorted_params = get_sorted_params(&params);
                let cache_key = (file_path.clone(), sorted_params);
                let cache = SSR_CACHE.lock().unwrap();
                cache.get(&cache_key).cloned()
            } else {
                None
            };

            let render_result = if let Some(html) = cached_html {
                Ok(html)
            } else {
                let content = fs::read_to_string(&file_path)?;
                let res = compiler::process_erm_component(file_path.to_str().unwrap(), &content, is_prod, &params);
                if let Ok(ref html) = res {
                    if is_prod {
                        let sorted_params = get_sorted_params(&params);
                        let cache_key = (file_path.clone(), sorted_params);
                        let mut cache = SSR_CACHE.lock().unwrap();
                        cache.insert(cache_key, html.clone());
                    }
                }
                res
            };

            match render_result {
                Ok(processed) => {
                    let rel_path = file_path.strip_prefix(&base_path)
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_else(|_| file_path.to_string_lossy().into_owned());
                    let mut processed = processed;
                    if !is_prod {
                        let filename_script = format!("<script>window.__erm_filename = \"{}\";</script>\n", rel_path);
                        if let Some(pos) = processed.find("<head>") {
                            processed.insert_str(pos + 6, &filename_script);
                        } else {
                            processed.insert_str(0, &filename_script);
                        }
                    }
                    unsafe {
                        let status = CString::new("200 OK").unwrap();
                        er_http_response_write_status(res, status.as_ptr(), status.as_bytes().len());
                        let content_type = CString::new("text/html; charset=utf-8").unwrap();
                        let content_type_key = CString::new("Content-Type").unwrap();
                        er_http_response_write_header(res, content_type_key.as_ptr(), content_type_key.as_bytes().len(), content_type.as_ptr(), content_type.as_bytes().len());
                        er_http_response_end(res, processed.as_ptr() as *const c_char, processed.len());
                    }
                }
                Err(e) => {
                    let rel_path = file_path.strip_prefix(&base_path)
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_else(|_| file_path.to_string_lossy().into_owned());

                    let escape_html = |s: &str| -> String {
                        s.replace('&', "&amp;")
                         .replace('<', "&lt;")
                         .replace('>', "&gt;")
                         .replace('"', "&quot;")
                         .replace('\'', "&#39;")
                    };

                    let err_msg_escaped = escape_html(&format!("{}", e));
                    let rel_path_escaped = escape_html(&rel_path);

                    let html_content = format!(r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>Compiler Error</title>
  <style>
    body {{
      background-color: #0b0b0d;
      margin: 0;
      padding: 0;
      min-height: 100vh;
    }}
  </style>
  <script src="/core/hmr.js"></script>
</head>
<body data-compile-error-file="{}" data-compile-error-message="{}">
</body>
</html>"#, rel_path_escaped, err_msg_escaped);

                    unsafe {
                        let status = CString::new("500 Internal Server Error").unwrap();
                        er_http_response_write_status(res, status.as_ptr(), status.as_bytes().len());
                        let content_type = CString::new("text/html; charset=utf-8").unwrap();
                        let content_type_key = CString::new("Content-Type").unwrap();
                        er_http_response_write_header(
                            res,
                            content_type_key.as_ptr(),
                            content_type_key.as_bytes().len(),
                            content_type.as_ptr(),
                            content_type.as_bytes().len(),
                        );
                        er_http_response_end(res, html_content.as_ptr() as *const c_char, html_content.len());
                    }
                }
            }
        } else if file_path.extension().map_or(false, |ext| ext == "er") {
            if let Err(e) = execute_api_route(res, method, target, headers, body, &file_path) {
                let err_msg = format!("Error running route: {}", e);
                unsafe {
                    let status = CString::new("500 Internal Server Error").unwrap();
                    er_http_response_write_status(res, status.as_ptr(), status.as_bytes().len());
                    er_http_response_end(res, err_msg.as_ptr() as *const c_char, err_msg.len());
                }
            }
        } else {
            let is_html = file_path.extension().map_or(false, |ext| ext == "html");
            let content = if is_prod && is_html {
                let mut cache = HTML_SHELL_CACHE.lock().unwrap();
                if let Some(cached) = cache.get(&file_path) {
                    cached.as_bytes().to_vec()
                } else {
                    let loaded = fs::read(&file_path)?;
                    if let Ok(loaded_str) = std::str::from_utf8(&loaded) {
                        cache.insert(file_path.clone(), loaded_str.to_string());
                    }
                    loaded
                }
            } else {
                fs::read(&file_path)?
            };
            let mime = if let Some(ext) = file_path.extension() {
                let ext_str = ext.to_string_lossy().to_lowercase();
                match ext_str.as_str() {
                    "html" => Some("text/html; charset=utf-8"),
                    "css" => Some("text/css; charset=utf-8"),
                    "js" => Some("application/javascript; charset=utf-8"),
                    "json" => Some("application/json; charset=utf-8"),
                    "png" => Some("image/png"),
                    "jpg" | "jpeg" => Some("image/jpeg"),
                    "gif" => Some("image/gif"),
                    "svg" => Some("image/svg+xml"),
                    "ico" => Some("image/x-icon"),
                    _ => None,
                }
            } else {
                None
            };
            unsafe {
                let status = CString::new("200 OK").unwrap();
                er_http_response_write_status(res, status.as_ptr(), status.as_bytes().len());
                if let Some(mime_type) = mime {
                    let content_type = CString::new(mime_type).unwrap();
                    let content_type_key = CString::new("Content-Type").unwrap();
                    er_http_response_write_header(res, content_type_key.as_ptr(), content_type_key.as_bytes().len(), content_type.as_ptr(), content_type.as_bytes().len());
                }
                er_http_response_end(res, content.as_ptr() as *const c_char, content.len());
            }
        }
    } else {
        unsafe {
            let status = CString::new("404 Not Found").unwrap();
            er_http_response_write_status(res, status.as_ptr(), status.as_bytes().len());
            let not_found = "Not Found";
            er_http_response_end(res, not_found.as_ptr() as *const c_char, not_found.len());
        }
    }
    Ok(())
}

pub fn start_server(dir: &str, is_prod: bool, port: u16) -> anyhow::Result<()> {
    let mut base_path = fs::canonicalize(dir)?;
    let mut default_file = None;

    if base_path.is_file() {
        default_file = Some(base_path.clone());
        if let Some(parent) = base_path.parent() {
            base_path = parent.to_path_buf();
        }
    } else {
        let config_path = base_path.join("config.er");
        if config_path.exists() {
            if let Ok(content) = fs::read_to_string(&config_path) {
                if let Ok(re) = regex::Regex::new(r#"(?s)server\s*:\s*\{[^}]*dev\s*:\s*["']([^"']+)["']"#) {
                    if let Some(caps) = re.captures(&content) {
                        if let Some(dev_val) = caps.get(1) {
                            let dev_file = base_path.join(dev_val.as_str());
                            if dev_file.exists() {
                                default_file = Some(dev_file);
                            }
                        }
                    }
                }
            }
        }
    }

    *BASE_PATH.lock().unwrap() = Some(base_path.clone());
    *DEFAULT_FILE.lock().unwrap() = default_file;
    *IS_PROD.lock().unwrap() = is_prod;

    unsafe {
        er_http_init_with_callbacks(
            dev_http_callback,
            dev_ws_open_callback,
            dev_ws_message_callback,
            dev_ws_close_callback,
        );
    }

    if !is_prod {
        let watch_path = base_path.clone();
        thread::spawn(move || {
            let mut last_files: HashMap<PathBuf, SystemTime> = HashMap::new();
            scan_directory(&watch_path, &mut last_files);
            let mut last_ping = SystemTime::now();

            loop {
                thread::sleep(Duration::from_millis(200));
                let mut current_files = HashMap::new();
                scan_directory(&watch_path, &mut current_files);

                let mut changed_file = None;

                for (path, mod_time) in &current_files {
                    match last_files.get(path) {
                        Some(last_time) => {
                            if mod_time > last_time {
                                changed_file = Some(path.clone());
                                break;
                            }
                        }
                        None => {
                            changed_file = Some(path.clone());
                            break;
                        }
                    }
                }

                if changed_file.is_none() {
                    for path in last_files.keys() {
                        if !current_files.contains_key(path) {
                            changed_file = Some(path.clone());
                            break;
                        }
                    }
                }

                if let Some(path) = changed_file {
                    last_files = current_files;
                    let rel_path = path.strip_prefix(&watch_path)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .into_owned();
                    
                    println!("[HMR] File changed: {}", rel_path);
                    
                    let path_str = if rel_path.starts_with('/') {
                        rel_path
                    } else {
                        format!("/{}", rel_path)
                    };
                    
                    let msg = serde_json::json!({
                        "type": "update",
                        "path": path_str
                    }).to_string();
                    
                    HMR_QUEUE.lock().unwrap().push(msg);
                    last_ping = SystemTime::now();
                } else {
                    last_files = current_files;
                    if let Ok(elapsed) = last_ping.elapsed() {
                        if elapsed >= Duration::from_secs(15) {
                            let ping = serde_json::json!({
                                "type": "ping"
                            }).to_string();
                            HMR_QUEUE.lock().unwrap().push(ping);
                            last_ping = SystemTime::now();
                        }
                    }
                }
            }
        });

        unsafe {
            let hmr_route = CString::new("/__hmr").unwrap();
            er_ws_register_route(hmr_route.as_ptr());
            er_http_create_timer(200, check_hmr_queue);
        }
    }

    println!("{} server running at http://localhost:{}", if is_prod { "Production" } else { "Dev" }, port);

    unsafe {
        er_http_listen_and_run(port as i32);
    }

    Ok(())
}

fn resolve_path(base_path: &Path, target: &str) -> Option<(PathBuf, HashMap<String, String>)> {
    let parts: Vec<&str> = target.split('/').filter(|s| !s.is_empty()).collect();
    let mut current_path = base_path.to_path_buf();
    let mut params = HashMap::new();

    for (i, part) in parts.iter().enumerate() {
        let mut found = false;
        
        let exact = current_path.join(part);
        if exact.exists() {
            current_path = exact;
            found = true;
        } else {
            let er = current_path.join(format!("{}.er", part));
            if er.exists() {
                current_path = er;
                found = true;
            } else {
                let erm = current_path.join(format!("{}.erm", part));
                if erm.exists() {
                    current_path = erm;
                    found = true;
                } else {
                    let html = current_path.join(format!("{}.html", part));
                    if html.exists() {
                        current_path = html;
                        found = true;
                    } else {
                        if let Ok(entries) = fs::read_dir(&current_path) {
                            for entry in entries.flatten() {
                                let name = entry.file_name().to_string_lossy().into_owned();
                                if name.starts_with('[') {
                                    if name.ends_with(']') {
                                        let param_name = &name[1..name.len() - 1];
                                        params.insert(param_name.to_string(), part.to_string());
                                        current_path.push(name);
                                        found = true;
                                        break;
                                    } else if name.ends_with("].er") {
                                        let param_name = &name[1..name.len() - 4];
                                        params.insert(param_name.to_string(), part.to_string());
                                        current_path.push(name);
                                        found = true;
                                        break;
                                    } else if name.ends_with("].erm") {
                                        let param_name = &name[1..name.len() - 5];
                                        params.insert(param_name.to_string(), part.to_string());
                                        current_path.push(name);
                                        found = true;
                                        break;
                                    } else if name.ends_with("].html") {
                                        let param_name = &name[1..name.len() - 6];
                                        params.insert(param_name.to_string(), part.to_string());
                                        current_path.push(name);
                                        found = true;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        if !found {
            let routes_er = current_path.join("routes.er");
            if routes_er.exists() {
                return Some((routes_er, params));
            }
            return None;
        }
        
        if current_path.is_file() && i < parts.len() - 1 {
            return None;
        }
    }

    if current_path.is_dir() {
        let routes_er = current_path.join("routes.er");
        if routes_er.exists() { return Some((routes_er, params)); }
        let index_erm = current_path.join("index.erm");
        if index_erm.exists() { return Some((index_erm, params)); }
        let page_erm = current_path.join("page.erm");
        if page_erm.exists() { return Some((page_erm, params)); }
        let index_html = current_path.join("index.html");
        if index_html.exists() { return Some((index_html, params)); }
        let page_html = current_path.join("page.html");
        if page_html.exists() { return Some((page_html, params)); }
    }

    Some((current_path, params))
}
