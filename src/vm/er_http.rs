use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use crate::vm::value::Value;
use crate::vm::execute::VM;
use crate::vm::gc::{get_or_create_string, gc_allocate, GcData};
use std::ffi::{c_char, c_void, CString};
use std::time::SystemTime;
use std::fs;
use std::path::Path;

pub struct Route {
    pub method: String,
    pub path: String,
    pub callback: Value,
}

pub struct WsRoute {
    pub path: String,
    pub open: Option<Value>,
    pub message: Option<Value>,
    pub close: Option<Value>,
}

thread_local! {
    pub static ROUTES: RefCell<Vec<Route>> = RefCell::new(Vec::new());
    pub static WS_ROUTES: RefCell<Vec<WsRoute>> = RefCell::new(Vec::new());
    pub static MIDDLEWARES: RefCell<Vec<Value>> = RefCell::new(Vec::new());
    pub static ACTIVE_VM: Cell<*mut VM> = const { Cell::new(std::ptr::null_mut()) };
    pub static ACTIVE_HTTP_RESPONSE: Cell<*mut c_void> = const { Cell::new(std::ptr::null_mut()) };
    pub static ACTIVE_WEBSOCKET: Cell<*mut c_void> = const { Cell::new(std::ptr::null_mut()) };
    pub static ACTIVE_CONNECTIONS: RefCell<HashMap<*mut c_void, Value>> = RefCell::new(HashMap::new());
    pub static ROUTE_PREFIX: RefCell<Option<String>> = const { RefCell::new(None) };
    static TARGET_SCRIPT_PATH: RefCell<Option<String>> = const { RefCell::new(None) };
    static LAST_MTIME: Cell<Option<SystemTime>> = const { Cell::new(None) };
    static LAST_CHECK_TIME: Cell<Option<SystemTime>> = const { Cell::new(None) };
    pub static LISTEN_PORT: Cell<Option<i32>> = const { Cell::new(None) };
    pub static LISTEN_CALLBACK: RefCell<Option<Value>> = const { RefCell::new(None) };
    pub static SERVER_RUNNING: Cell<bool> = const { Cell::new(false) };
}

unsafe extern "C" {
    fn er_http_init();
    fn er_http_register_route(method: *const c_char, path: *const c_char);
    fn er_http_listen_and_run(port: i32);
    fn er_http_response_end_json(res: *mut c_void, json_str: *const c_char, json_len: usize);
    fn er_http_response_end_html(res: *mut c_void, html_str: *const c_char, html_len: usize);
    
    fn er_ws_register_route(path: *const c_char);
    fn er_ws_send(ws: *mut c_void, message: *const c_char, message_len: usize);
    fn er_ws_close(ws: *mut c_void);

    fn er_http_create_timer(ms: i32, cb: extern "C" fn(*mut c_void));
}

pub fn native_route(_args: Vec<Value>) -> Value {
    let router_obj = crate::vm::gc::get_pooled_map(5);
    
    let get_name = get_or_create_string("get");
    let post_name = get_or_create_string("post");
    let ws_name = get_or_create_string("ws");
    let use_name = get_or_create_string("use");
    let listen_name = get_or_create_string("listen");
    
    let get_fn = Value::native_function(native_router_get);
    let post_fn = Value::native_function(native_router_post);
    let ws_fn = Value::native_function(native_router_ws);
    let use_fn = Value::native_function(native_router_use);
    let listen_fn = Value::native_function(native_router_listen);
    
    let mut map = router_obj;
    map.insert(crate::vm::value::MapKey(Value::string(get_name)), get_fn);
    map.insert(crate::vm::value::MapKey(Value::string(post_name)), post_fn);
    map.insert(crate::vm::value::MapKey(Value::string(ws_name)), ws_fn);
    map.insert(crate::vm::value::MapKey(Value::string(use_name)), use_fn);
    map.insert(crate::vm::value::MapKey(Value::string(listen_name)), listen_fn);
    
    let ptr = gc_allocate(GcData::Object(map));
    Value::object(ptr)
}

pub fn native_router_listen(args: Vec<Value>) -> Value {
    let mut port_val = Value::null();
    if args.len() >= 1 {
        port_val = args[0];
        if port_val.is_number() {
            LISTEN_PORT.with(|port| {
                port.set(Some(port_val.as_number() as i32));
            });
        }
    }
    if args.len() >= 2 {
        let callback_val = args[1];
        if callback_val.is_function() || callback_val.is_native_function() {
            LISTEN_CALLBACK.with(|cb| {
                *cb.borrow_mut() = Some(callback_val);
            });
            let is_running = SERVER_RUNNING.with(|r| r.get());
            if is_running {
                let vm_ptr = ACTIVE_VM.with(|active| active.get());
                if !vm_ptr.is_null() {
                    let vm = unsafe { &mut *vm_ptr };
                    let mut cb_args = Vec::new();
                    if !port_val.is_null() {
                        cb_args.push(port_val);
                    }
                    if let Err(e) = vm.call_function_reentrant(callback_val, cb_args) {
                        eprintln!("[HTTP] Error running listen callback: {}", e);
                    }
                }
            }
        }
    }
    Value::null()
}

pub fn native_router_use(args: Vec<Value>) -> Value {
    if args.len() >= 1 {
        let callback = args[0];
        MIDDLEWARES.with(|mws| {
            mws.borrow_mut().push(callback);
        });
    }
    Value::null()
}

pub fn native_router_ws(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::null();
    }
    let path_val = args[0];
    let callbacks_obj = args[1];
    
    if !path_val.is_string() {
        eprintln!("[WS] Error: WebSocket path must be a string");
        return Value::null();
    }
    if !callbacks_obj.is_object() {
        eprintln!("[WS] Error: WebSocket callbacks must be an object");
        return Value::null();
    }
    
    let path_str = unsafe {
        match &(*path_val.as_gc_ptr()).data {
            GcData::String(s) => s.as_ref().to_string(),
            _ => return Value::null(),
        }
    };
    
    let open_name = get_or_create_string("open");
    let message_name = get_or_create_string("message");
    let close_name = get_or_create_string("close");
    
    let open_val = get_property_helper(callbacks_obj, Value::string(open_name));
    let message_val = get_property_helper(callbacks_obj, Value::string(message_name));
    let close_val = get_property_helper(callbacks_obj, Value::string(close_name));
    
    let open_cb = if open_val.is_function() { Some(open_val) } else { None };
    let message_cb = if message_val.is_function() { Some(message_val) } else { None };
    let close_cb = if close_val.is_function() { Some(close_val) } else { None };
    
    WS_ROUTES.with(|routes| {
        routes.borrow_mut().push(WsRoute {
            path: path_str,
            open: open_cb,
            message: message_cb,
            close: close_cb,
        });
    });
    
    Value::null()
}

pub fn native_ws_send(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let message_val = args[0];
    let message_str = if message_val.is_string() {
        unsafe {
            match &(*message_val.as_gc_ptr()).data {
                GcData::String(s) => s.as_ref().to_string(),
                _ => return Value::null(),
            }
        }
    } else {
        message_val.to_string()
    };
    
    ACTIVE_WEBSOCKET.with(|active| {
        let ptr = active.get();
        if !ptr.is_null() {
            let c_str = CString::new(message_str).unwrap();
            unsafe {
                er_ws_send(ptr, c_str.as_ptr(), c_str.as_bytes().len());
            }
        } else {
            eprintln!("[WS] Error: ACTIVE_WEBSOCKET is null when calling send()");
        }
    });
    
    Value::null()
}

pub fn native_ws_close(_args: Vec<Value>) -> Value {
    ACTIVE_WEBSOCKET.with(|active| {
        let ptr = active.get();
        if !ptr.is_null() {
            unsafe {
                er_ws_close(ptr);
            }
        } else {
            eprintln!("[WS] Error: ACTIVE_WEBSOCKET is null when calling close()");
        }
    });
    Value::null()
}

fn create_ws_object(_ws: *mut c_void) -> Value {
    let ws_map = crate::vm::gc::get_pooled_map(2);
    
    let send_name = get_or_create_string("send");
    let send_fn = Value::native_function(native_ws_send);
    
    let close_name = get_or_create_string("close");
    let close_fn = Value::native_function(native_ws_close);
    
    let mut map = ws_map;
    map.insert(crate::vm::value::MapKey(Value::string(send_name)), send_fn);
    map.insert(crate::vm::value::MapKey(Value::string(close_name)), close_fn);
    
    let ptr = gc_allocate(GcData::Object(map));
    Value::object(ptr)
}

fn register_route_internal(method: &str, args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::null();
    }
    let path_val = args[0];
    let callback_val = args[1];
    
    if !path_val.is_string() {
        eprintln!("[HTTP] Error: Route path must be a string");
        return Value::null();
    }
    if !callback_val.is_function() {
        eprintln!("[HTTP] Error: Route callback must be a function");
        return Value::null();
    }
    
    let mut path_str = unsafe {
        match &(*path_val.as_gc_ptr()).data {
            GcData::String(s) => s.as_ref().to_string(),
            _ => return Value::null(),
        }
    };
    
    ROUTE_PREFIX.with(|prefix| {
        if let Some(ref p) = *prefix.borrow() {
            let already_prepended = path_str == *p || path_str.starts_with(&format!("{}/", p));
            if !already_prepended {
                if path_str == "/" {
                    path_str = p.clone();
                } else if path_str.starts_with('/') {
                    path_str = format!("{}{}", p, path_str);
                } else {
                    path_str = format!("{}/{}", p, path_str);
                }
            }
        }
    });
    
    ROUTES.with(|routes| {
        routes.borrow_mut().push(Route {
            method: method.to_string(),
            path: path_str,
            callback: callback_val,
        });
    });
    
    Value::null()
}

pub fn native_router_get(args: Vec<Value>) -> Value {
    register_route_internal("GET", args)
}

pub fn native_router_post(args: Vec<Value>) -> Value {
    register_route_internal("POST", args)
}

pub fn native_context_json(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let data = args[0];
    let json_val = value_to_json(data);
    let json_str = serde_json::to_string(&json_val).unwrap_or_else(|_| "null".to_string());
    
    ACTIVE_HTTP_RESPONSE.with(|resp| {
        let ptr = resp.get();
        if !ptr.is_null() {
            let c_str = CString::new(json_str).unwrap();
            unsafe {
                er_http_response_end_json(ptr, c_str.as_ptr(), c_str.as_bytes().len());
            }
        } else {
            eprintln!("[HTTP] Error: ACTIVE_HTTP_RESPONSE is null");
        }
    });
    
    Value::null()
}

pub fn native_context_html(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let html_val = args[0];
    let html_str = if html_val.is_string() {
        unsafe {
            match &(*html_val.as_gc_ptr()).data {
                GcData::String(s) => s.as_ref().to_string(),
                _ => return Value::null(),
            }
        }
    } else {
        html_val.to_string()
    };
    
    ACTIVE_HTTP_RESPONSE.with(|resp| {
        let ptr = resp.get();
        if !ptr.is_null() {
            let c_str = CString::new(html_str).unwrap();
            unsafe {
                er_http_response_end_html(ptr, c_str.as_ptr(), c_str.as_bytes().len());
            }
        } else {
            eprintln!("[HTTP] Error: ACTIVE_HTTP_RESPONSE is null");
        }
    });
    
    Value::null()
}

pub fn get_target_script_path() -> Option<String> {
    TARGET_SCRIPT_PATH.with(|p| p.borrow().clone())
}


pub fn end_http_response_json(res: *mut std::ffi::c_void, json: &str) {
    let c_str = std::ffi::CString::new(json).unwrap();
    unsafe {
        er_http_response_end_json(res, c_str.as_ptr(), c_str.as_bytes().len());
    }
}

pub fn value_to_json(val: Value) -> serde_json::Value {
    if val.is_null() {
        serde_json::Value::Null
    } else if val.is_boolean() {
        serde_json::Value::Bool(val.as_boolean())
    } else if val.is_number() {
        let num = val.as_number();
        if let Some(n) = serde_json::Number::from_f64(num) {
            serde_json::Value::Number(n)
        } else {
            serde_json::Value::Null
        }
    } else if val.is_string() {
        val.as_str().map(|s| serde_json::Value::String(s.to_string())).unwrap_or(serde_json::Value::Null)
    } else if val.is_array() {
        unsafe {
            match &(*val.as_gc_ptr()).data {
                GcData::Array(arr) => {
                    let items: Vec<serde_json::Value> = arr.iter().map(|&v| value_to_json(v)).collect();
                    serde_json::Value::Array(items)
                }
                _ => serde_json::Value::Null
            }
        }
    } else if val.is_object() {
        unsafe {
            match &(*val.as_gc_ptr()).data {
                GcData::Object(map) => {
                    let mut obj = serde_json::Map::new();
                    for (k, v) in map {
                        let key_str = match &(*k.0.as_gc_ptr()).data {
                            GcData::String(s) => s.as_ref().to_string(),
                            _ => continue,
                        };
                        obj.insert(key_str, value_to_json(*v));
                    }
                    serde_json::Value::Object(obj)
                }
                GcData::Struct(s) => {
                    let mut obj = serde_json::Map::new();
                    for (map_key, &idx) in &s.descriptor.field_indices {
                        let name = map_key.0.as_str().unwrap_or("");
                        obj.insert(name.to_string(), value_to_json(s.fields[idx]));
                    }
                    serde_json::Value::Object(obj)
                }
                _ => serde_json::Value::Null
            }
        }
    } else {
        serde_json::Value::Null
    }
}

fn get_property_helper(obj: Value, name_val: Value) -> Value {
    if obj.is_object() {
        let ptr = obj.as_gc_ptr();
        unsafe {
            match &(*ptr).data {
                GcData::Object(map) => {
                    return map.get(&crate::vm::value::MapKey(name_val)).cloned().unwrap_or(Value::null());
                }
                GcData::Struct(s) => {
                    return s.get_field(name_val).unwrap_or(Value::null());
                }
                _ => {}
            }
        }
    }
    Value::null()
}

fn get_port_from_config(vm: &VM) -> i32 {
    if let Some(&config_val) = vm.get_global("config") {
        if config_val.is_object() {
            let server_name = get_or_create_string("server");
            let server_val = get_property_helper(config_val, Value::string(server_name));
            if server_val.is_object() {
                let port_name = get_or_create_string("port");
                let port_val = get_property_helper(server_val, Value::string(port_name));
                if port_val.is_number() {
                    return port_val.as_number() as i32;
                }
            }
        }
    }
    3000
}

pub fn start_http_server_if_needed(vm: &mut VM) {
    let has_http_routes = ROUTES.with(|r| !r.borrow().is_empty());
    let has_ws_routes = WS_ROUTES.with(|r| !r.borrow().is_empty());
    let has_listen = LISTEN_PORT.with(|p| p.get().is_some());
    if !has_http_routes && !has_ws_routes && !has_listen {
        return;
    }
    
    let port = LISTEN_PORT.with(|p| p.get()).unwrap_or_else(|| get_port_from_config(vm));
    println!("[HTTP] Starting uWebSockets HTTP server on port {}...", port);
    
    unsafe {
        er_http_init();
    }
    
    ROUTES.with(|routes| {
        for route in routes.borrow().iter() {
            let method_c = CString::new(route.method.as_str()).unwrap();
            let path_c = CString::new(route.path.as_str()).unwrap();
            unsafe {
                er_http_register_route(method_c.as_ptr(), path_c.as_ptr());
            }
        }
    });
    
    WS_ROUTES.with(|routes| {
        for route in routes.borrow().iter() {
            let path_c = CString::new(route.path.as_str()).unwrap();
            unsafe {
                er_ws_register_route(path_c.as_ptr());
            }
        }
    });
    
    crate::vm::gc::GC_ROOTS.with(|roots| {
        roots.borrow_mut().push(Box::new(|| {
            ROUTES.with(|routes| {
                for route in routes.borrow().iter() {
                    crate::vm::gc::mark_value(&route.callback);
                }
            });
            MIDDLEWARES.with(|mws| {
                for mw in mws.borrow().iter() {
                    crate::vm::gc::mark_value(mw);
                }
            });
            WS_ROUTES.with(|routes| {
                for route in routes.borrow().iter() {
                    if let Some(open_cb) = &route.open {
                        crate::vm::gc::mark_value(open_cb);
                    }
                    if let Some(msg_cb) = &route.message {
                        crate::vm::gc::mark_value(msg_cb);
                    }
                    if let Some(close_cb) = &route.close {
                        crate::vm::gc::mark_value(close_cb);
                    }
                }
            });
            ACTIVE_CONNECTIONS.with(|conns| {
                for &ws_obj in conns.borrow().values() {
                    crate::vm::gc::mark_value(&ws_obj);
                }
            });
            LISTEN_CALLBACK.with(|cb| {
                if let Some(callback) = &*cb.borrow() {
                    crate::vm::gc::mark_value(callback);
                }
            });
        }));
    });
    
    ACTIVE_VM.with(|active| {
        active.set(vm as *mut VM);
    });
    
    SERVER_RUNNING.with(|r| r.set(true));
    unsafe {
        er_http_create_timer(1, er_http_on_timer);
        er_http_listen_and_run(port);
    }
    
    ACTIVE_VM.with(|active| {
        active.set(std::ptr::null_mut());
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn er_http_on_timer(_timer: *mut c_void) {
    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if !vm_ptr.is_null() {
        let vm = unsafe { &mut *vm_ptr };
        let _ = vm.run_event_loop();
    }
}

pub fn set_target_script_path(path: &str) {
    TARGET_SCRIPT_PATH.with(|p| {
        *p.borrow_mut() = Some(path.to_string());
    });
    let mtime = get_max_mtime_for_reload(path);
    LAST_MTIME.with(|m| {
        m.set(mtime);
    });
}

fn get_max_mtime_for_reload(path: &str) -> Option<SystemTime> {
    let mut max_mtime = fs::metadata(path).ok()?.modified().ok()?;
    
    let path_obj = Path::new(path);
    if let Some(parent) = path_obj.parent() {
        let config_path = parent.join("config.er");
        if config_path.exists() {
            if let Ok(meta) = fs::metadata(&config_path) {
                if let Ok(mtime) = meta.modified() {
                    if mtime > max_mtime {
                        max_mtime = mtime;
                    }
                }
            }
        }
    }
    
    Some(max_mtime)
}

fn check_and_reload_script_if_needed(vm: &mut VM) {
    let now = SystemTime::now();
    let should_check = LAST_CHECK_TIME.with(|last_check| {
        if let Some(last) = last_check.get() {
            if let Ok(elapsed) = now.duration_since(last) {
                if elapsed.as_millis() < 500 {
                    return false;
                }
            }
        }
        last_check.set(Some(now));
        true
    });
    
    if !should_check {
        return;
    }

    let script_path = TARGET_SCRIPT_PATH.with(|p| p.borrow().clone());
    let Some(path) = script_path else {
        return;
    };
    
    let current_mtime = match get_max_mtime_for_reload(&path) {
        Some(mtime) => mtime,
        None => return,
    };
    
    let last_mtime = LAST_MTIME.with(|m| m.get());
    if Some(current_mtime) == last_mtime {
        return;
    }
    
    println!("[HTTP] File change detected, reloading script: {}...", path);
    
    let old_routes = ROUTES.with(|r| std::mem::take(&mut *r.borrow_mut()));
    let old_ws_routes = WS_ROUTES.with(|r| std::mem::take(&mut *r.borrow_mut()));
    let old_mws = MIDDLEWARES.with(|r| std::mem::take(&mut *r.borrow_mut()));
    let old_listen_port = LISTEN_PORT.with(|p| p.replace(None));
    let old_listen_callback = LISTEN_CALLBACK.with(|cb| cb.replace(None));
    
    let path_buf = Path::new(&path);
    let stmts = match crate::frontend::parse_and_resolve_imports(path_buf) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[HTTP] Reload error: Parsing/Import resolution failed: {}", e);
            ROUTES.with(|r| *r.borrow_mut() = old_routes);
            WS_ROUTES.with(|r| *r.borrow_mut() = old_ws_routes);
            MIDDLEWARES.with(|r| *r.borrow_mut() = old_mws);
            LISTEN_PORT.with(|p| p.set(old_listen_port));
            LISTEN_CALLBACK.with(|cb| *cb.borrow_mut() = old_listen_callback);
            return;
        }
    };
    
    let compiler = crate::vm::compiler::Compiler::new();
    let function = match compiler.compile(&stmts) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[HTTP] Reload error: Compilation failed: {}", e);
            ROUTES.with(|r| *r.borrow_mut() = old_routes);
            WS_ROUTES.with(|r| *r.borrow_mut() = old_ws_routes);
            MIDDLEWARES.with(|r| *r.borrow_mut() = old_mws);
            LISTEN_PORT.with(|p| p.set(old_listen_port));
            LISTEN_CALLBACK.with(|cb| *cb.borrow_mut() = old_listen_callback);
            return;
        }
    };
    
    // Reload config.er if it exists
    let parent_dir = Path::new(&path).parent();
    if let Some(parent) = parent_dir {
        let config_path = parent.join("config.er");
        if config_path.exists() {
            if let Ok(config_content) = fs::read_to_string(&config_path) {
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
        eprintln!("[HTTP] Reload error: Execution failed: {}", e);
        ROUTES.with(|r| *r.borrow_mut() = old_routes);
        WS_ROUTES.with(|r| *r.borrow_mut() = old_ws_routes);
        MIDDLEWARES.with(|r| *r.borrow_mut() = old_mws);
        LISTEN_PORT.with(|p| p.set(old_listen_port));
        LISTEN_CALLBACK.with(|cb| *cb.borrow_mut() = old_listen_callback);
        return;
    }
    
    LAST_MTIME.with(|m| m.set(Some(current_mtime)));
    println!("[HTTP] Reload successful. VM state and routes updated.");
}

fn match_route_path(pattern: &str, path: &str) -> Option<HashMap<String, String>> {
    let pattern_parts: Vec<&str> = pattern.split('/').collect();
    let path_parts: Vec<&str> = path.split('/').collect();
    
    if pattern_parts.len() != path_parts.len() {
        return None;
    }
    
    let mut params = HashMap::new();
    for (pat_part, path_part) in pattern_parts.iter().zip(path_parts.iter()) {
        if pat_part.starts_with(':') {
            let param_name = &pat_part[1..];
            params.insert(param_name.to_string(), path_part.to_string());
        } else if pat_part != path_part {
            return None;
        }
    }
    
    Some(params)
}

#[unsafe(no_mangle)]
pub extern "C" fn er_http_on_listening() {
    let cb_opt = LISTEN_CALLBACK.with(|cb| cb.borrow().clone());
    if let Some(callback) = cb_opt {
        ACTIVE_VM.with(|active| {
            let vm_ptr = active.get();
            if !vm_ptr.is_null() {
                let vm = unsafe { &mut *vm_ptr };
                if let Err(e) = vm.call_function_reentrant(callback, vec![]) {
                    eprintln!("[HTTP] Error executing listen callback: {}", e);
                }
                if let Err(e) = vm.run_event_loop() {
                    eprintln!("[HTTP] Event loop error in listen callback: {}", e);
                }
            }
        });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_http_on_request(
    res: *mut c_void,
    method_ptr: *const c_char,
    method_len: usize,
    path_ptr: *const c_char,
    path_len: usize,
) {
    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if !vm_ptr.is_null() {
        let vm = unsafe { &mut *vm_ptr };
        check_and_reload_script_if_needed(vm);
    }

    let method = unsafe {
        let slice = std::slice::from_raw_parts(method_ptr as *const u8, method_len);
        std::str::from_utf8(slice).unwrap_or("")
    };
    let path = unsafe {
        let slice = std::slice::from_raw_parts(path_ptr as *const u8, path_len);
        std::str::from_utf8(slice).unwrap_or("")
    };
    
    let mut extracted_params = HashMap::new();
    let callback_opt = ROUTES.with(|routes| {
        for route in routes.borrow().iter() {
            if route.method == method {
                if let Some(params) = match_route_path(&route.path, path) {
                    extracted_params = params;
                    return Some(route.callback);
                }
            }
        }
        None
    });
    
    if let Some(callback) = callback_opt {
        ACTIVE_HTTP_RESPONSE.with(|resp| {
            resp.set(res);
        });
        
        ACTIVE_VM.with(|active| {
            let vm_ptr = active.get();
            if !vm_ptr.is_null() {
                let vm = unsafe { &mut *vm_ptr };
                
                let mut req_map = crate::vm::gc::get_pooled_map(3);
                let url_name = get_or_create_string("url");
                let method_name = get_or_create_string("method");
                let params_name = get_or_create_string("params");
                let path_str = get_or_create_string(path);
                let method_str = get_or_create_string(method);
                
                let mut params_obj_map = crate::vm::gc::get_pooled_map(extracted_params.len());
                for (k, v) in extracted_params {
                    let k_str = get_or_create_string(&k);
                    let v_str = get_or_create_string(&v);
                    params_obj_map.insert(crate::vm::value::MapKey(Value::string(k_str)), Value::string(v_str));
                }
                let params_obj = Value::object(crate::vm::gc::gc_allocate(GcData::Object(params_obj_map)));
                
                req_map.insert(crate::vm::value::MapKey(Value::string(url_name)), Value::string(path_str));
                req_map.insert(crate::vm::value::MapKey(Value::string(method_name)), Value::string(method_str));
                req_map.insert(crate::vm::value::MapKey(Value::string(params_name)), params_obj);
                
                let req_obj = Value::object(crate::vm::gc::gc_allocate(GcData::Object(req_map)));
                
                let context_obj = crate::vm::gc::get_pooled_map(3);
                let json_name = get_or_create_string("json");
                let json_val = Value(crate::vm::value::TAG_METHOD_SEND_JSON | (res as u64 & crate::vm::value::PTR_MASK));
                let html_name = get_or_create_string("html");
                let html_val = Value::native_function(native_context_html);
                let req_key_name = get_or_create_string("req");
                
                let mut map = context_obj;
                map.insert(crate::vm::value::MapKey(Value::string(json_name)), json_val);
                map.insert(crate::vm::value::MapKey(Value::string(html_name)), html_val);
                map.insert(crate::vm::value::MapKey(Value::string(req_key_name)), req_obj);
                let c_val = Value::object(crate::vm::gc::gc_allocate(GcData::Object(map)));
                
                
                let mws = MIDDLEWARES.with(|m| m.borrow().clone());
                let mut mw_err = false;
                for mw in mws {
                    if let Err(e) = vm.call_function_reentrant(mw, vec![c_val]) {
                        eprintln!("[HTTP] Error executing middleware: {}", e);
                        mw_err = true;
                        break;
                    }
                }
                
                if !mw_err {
                    if let Err(e) = vm.call_function_reentrant(callback, vec![c_val]) {
                        eprintln!("[HTTP] Error executing callback: {}", e);
                    }
                }

                if let Err(e) = vm.run_event_loop() {
                    eprintln!("[HTTP] Event loop error: {}", e);
                }
            } else {
                eprintln!("[HTTP] Error: ACTIVE_VM is null during request handler");
            }
        });
        
        ACTIVE_HTTP_RESPONSE.with(|resp| {
            resp.set(std::ptr::null_mut());
        });
    } else {
        unsafe {
            let c_str = CString::new("{\"error\": \"Not Found\"}").unwrap();
            er_http_response_end_json(res, c_str.as_ptr(), c_str.as_bytes().len());
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_ws_on_open(
    ws: *mut c_void,
    path_ptr: *const c_char,
    path_len: usize,
) {
    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if !vm_ptr.is_null() {
        let vm = unsafe { &mut *vm_ptr };
        check_and_reload_script_if_needed(vm);
    }

    let path = unsafe {
        let slice = std::slice::from_raw_parts(path_ptr as *const u8, path_len);
        std::str::from_utf8(slice).unwrap_or("")
    };
    
    let open_cb = WS_ROUTES.with(|routes| {
        for route in routes.borrow().iter() {
            if route.path == path {
                return route.open;
            }
        }
        None
    });
    
    let ws_obj = ACTIVE_CONNECTIONS.with(|conns| {
        let mut cache = conns.borrow_mut();
        if let Some(&cached) = cache.get(&ws) {
            cached
        } else {
            let obj = create_ws_object(ws);
            cache.insert(ws, obj);
            obj
        }
    });

    if let Some(callback) = open_cb {
        ACTIVE_WEBSOCKET.with(|active| active.set(ws));
        
        ACTIVE_VM.with(|active| {
            let vm_ptr = active.get();
            if !vm_ptr.is_null() {
                let vm = unsafe { &mut *vm_ptr };
                if let Err(e) = vm.call_function_reentrant(callback, vec![ws_obj]) {
                    eprintln!("[WS] Error executing open callback: {}", e);
                }
            }
        });
        
        ACTIVE_WEBSOCKET.with(|active| active.set(std::ptr::null_mut()));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_ws_on_message(
    ws: *mut c_void,
    path_ptr: *const c_char,
    path_len: usize,
    message_ptr: *const c_char,
    message_len: usize,
) {
    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if !vm_ptr.is_null() {
        let vm = unsafe { &mut *vm_ptr };
        check_and_reload_script_if_needed(vm);
    }

    let path = unsafe {
        let slice = std::slice::from_raw_parts(path_ptr as *const u8, path_len);
        std::str::from_utf8(slice).unwrap_or("")
    };
    
    let msg = unsafe {
        let slice = std::slice::from_raw_parts(message_ptr as *const u8, message_len);
        std::str::from_utf8(slice).unwrap_or("")
    };
    
    let message_cb = WS_ROUTES.with(|routes| {
        for route in routes.borrow().iter() {
            if route.path == path {
                return route.message;
            }
        }
        None
    });
    
    let ws_obj = ACTIVE_CONNECTIONS.with(|conns| {
        let mut cache = conns.borrow_mut();
        if let Some(&cached) = cache.get(&ws) {
            cached
        } else {
            let obj = create_ws_object(ws);
            cache.insert(ws, obj);
            obj
        }
    });

    if let Some(callback) = message_cb {
        ACTIVE_WEBSOCKET.with(|active| active.set(ws));
        
        ACTIVE_VM.with(|active| {
            let vm_ptr = active.get();
            if !vm_ptr.is_null() {
                let vm = unsafe { &mut *vm_ptr };
                let msg_str = get_or_create_string(msg);
                let msg_val = Value::string(msg_str);
                
                if let Err(e) = vm.call_function_reentrant(callback, vec![ws_obj, msg_val]) {
                    eprintln!("[WS] Error executing message callback: {}", e);
                }
            }
        });
        
        ACTIVE_WEBSOCKET.with(|active| active.set(std::ptr::null_mut()));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn er_ws_on_close(
    ws: *mut c_void,
    path_ptr: *const c_char,
    path_len: usize,
    code: i32,
    message_ptr: *const c_char,
    message_len: usize,
) {
    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if !vm_ptr.is_null() {
        let vm = unsafe { &mut *vm_ptr };
        check_and_reload_script_if_needed(vm);
    }

    let path = unsafe {
        let slice = std::slice::from_raw_parts(path_ptr as *const u8, path_len);
        std::str::from_utf8(slice).unwrap_or("")
    };
    
    let msg = unsafe {
        let slice = std::slice::from_raw_parts(message_ptr as *const u8, message_len);
        std::str::from_utf8(slice).unwrap_or("")
    };
    
    let close_cb = WS_ROUTES.with(|routes| {
        for route in routes.borrow().iter() {
            if route.path == path {
                return route.close;
            }
        }
        None
    });
    
    let ws_obj = ACTIVE_CONNECTIONS.with(|conns| {
        let mut cache = conns.borrow_mut();
        cache.remove(&ws).unwrap_or_else(|| create_ws_object(ws))
    });

    if let Some(callback) = close_cb {
        ACTIVE_WEBSOCKET.with(|active| active.set(ws));
        
        ACTIVE_VM.with(|active| {
            let vm_ptr = active.get();
            if !vm_ptr.is_null() {
                let vm = unsafe { &mut *vm_ptr };
                let code_val = Value::number(code as f64);
                let msg_str = get_or_create_string(msg);
                let msg_val = Value::string(msg_str);
                
                if let Err(e) = vm.call_function_reentrant(callback, vec![ws_obj, code_val, msg_val]) {
                    eprintln!("[WS] Error executing close callback: {}", e);
                }
            }
        });
        
        ACTIVE_WEBSOCKET.with(|active| active.set(std::ptr::null_mut()));
    }
}

pub fn native_fetch(args: Vec<Value>) -> Value {
    if args.len() >= 2 && (args[1].is_function() || args[1].is_native_function()) {
        return native_fetch_async(args);
    }
    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() {
        eprintln!("[Fetch] Error: ACTIVE_VM is null");
        return Value::null();
    }
    let vm = unsafe { &*vm_ptr };
    if vm.use_evented_io {
        native_fetch_evented(args)
    } else {
        native_fetch_sync(args)
    }
}

pub fn native_set_timeout(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        eprintln!("[setTimeout] Error: callback and delay are required");
        return Value::null();
    }
    let callback = args[0];
    let delay_val = args[1];
    if !callback.is_function() && !callback.is_native_function() {
        eprintln!("[setTimeout] Error: first argument must be a function");
        return Value::null();
    }
    if !delay_val.is_number() {
        eprintln!("[setTimeout] Error: second argument must be a number");
        return Value::null();
    }
    let delay_ms = delay_val.as_number() as u64;

    let mut cb_args = Vec::new();
    if args.len() > 2 {
        cb_args.extend_from_slice(&args[2..]);
    }

    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() {
        eprintln!("[setTimeout] Error: ACTIVE_VM is null");
        return Value::null();
    }
    let vm = unsafe { &mut *vm_ptr };

    let queue = vm.event_loop_queue.clone();
    let active_counter = vm.active_async_tasks.clone();
    let pending = vm.pending_callbacks.clone();

    pending.lock().unwrap().push(crate::vm::execute::PendingAsync {
        callback,
        args: cb_args.clone(),
    });
    active_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));

        let mut q = queue.lock().unwrap();
        q.push(crate::vm::execute::EventLoopTask {
            callback,
            args: cb_args,
            result: crate::vm::execute::AsyncResult::Timeout,
        });

        let mut p = pending.lock().unwrap();
        if let Some(pos) = p.iter().position(|x| x.callback.0 == callback.0) {
            p.remove(pos);
        }

        active_counter.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    });

    Value::null()
}

use std::sync::OnceLock;

fn get_http_agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .user_agent("Eronom/0.1.0")
            .build()
    })
}

pub fn native_fetch_async(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        eprintln!("[fetchAsync] Error: URL and callback are required");
        return Value::null();
    }
    let url_val = args[0];
    let callback = args[1];
    if !url_val.is_string() {
        eprintln!("[fetchAsync] Error: URL must be a string");
        return Value::null();
    }
    if !callback.is_function() && !callback.is_native_function() {
        eprintln!("[fetchAsync] Error: callback must be a function");
        return Value::null();
    }
    let url_str = unsafe {
        match &(*url_val.as_gc_ptr()).data {
            GcData::String(s) => s.as_ref().to_string(),
            _ => return Value::null(),
        }
    };

    let mut cb_args = Vec::new();
    if args.len() > 2 {
        cb_args.extend_from_slice(&args[2..]);
    }

    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() {
        eprintln!("[fetchAsync] Error: ACTIVE_VM is null");
        return Value::null();
    }
    let vm = unsafe { &mut *vm_ptr };

    let queue = vm.event_loop_queue.clone();
    let active_counter = vm.active_async_tasks.clone();
    let pending = vm.pending_callbacks.clone();

    pending.lock().unwrap().push(crate::vm::execute::PendingAsync {
        callback,
        args: cb_args.clone(),
    });
    active_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    std::thread::spawn(move || {
        let agent = get_http_agent();
        let res = match agent.get(&url_str).call() {
            Ok(resp) => resp.into_string().map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        };

        let mut q = queue.lock().unwrap();
        q.push(crate::vm::execute::EventLoopTask {
            callback,
            args: cb_args,
            result: crate::vm::execute::AsyncResult::Fetch(res),
        });

        let mut p = pending.lock().unwrap();
        if let Some(pos) = p.iter().position(|x| x.callback.0 == callback.0) {
            p.remove(pos);
        }

        active_counter.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    });

    Value::null()
}

pub fn native_fetch_sync(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let url_val = args[0];
    if !url_val.is_string() {
        eprintln!("[FetchSync] Error: URL must be a string");
        return Value::null();
    }
    let url_str = unsafe {
        match &(*url_val.as_gc_ptr()).data {
            GcData::String(s) => s.as_ref().to_string(),
            _ => return Value::null(),
        }
    };

    let agent = get_http_agent();
    match agent.get(&url_str).call() {
        Ok(resp) => {
            match resp.into_string() {
                Ok(body_str) => {
                    let mut map = crate::vm::gc::get_pooled_map(2);
                    let body_key = crate::vm::gc::get_or_create_string("_body");
                    let body_val = crate::vm::gc::get_or_create_string(&body_str);
                    map.insert(crate::vm::value::MapKey(Value::string(body_key)), Value::string(body_val));
                    let ptr = crate::vm::gc::gc_allocate(crate::vm::gc::GcData::Object(map));
                    Value::object(ptr)
                }
                Err(e) => {
                    eprintln!("[FetchSync] Error reading body: {}", e);
                    Value::null()
                }
            }
        }
        Err(e) => {
            eprintln!("[FetchSync] Error: {}", e);
            Value::null()
        }
    }
}

pub fn native_fetch_evented(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let url_val = args[0];
    if !url_val.is_string() {
        eprintln!("[FetchEvented] Error: URL must be a string");
        return Value::null();
    }
    let url_str = unsafe {
        match &(*url_val.as_gc_ptr()).data {
            GcData::String(s) => s.as_ref().to_string(),
            _ => return Value::null(),
        }
    };

    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() {
        eprintln!("[FetchEvented] Error: ACTIVE_VM is null");
        return Value::null();
    }
    let vm = unsafe { &mut *vm_ptr };

    // 1. Create a promise
    let state = std::sync::Arc::new(std::sync::Mutex::new(crate::vm::gc::PromiseState::Pending));
    let prom = crate::vm::gc::GcPromise {
        state: state.clone(),
        suspended_stack: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        suspended_frames: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
    };
    let promise_ptr = crate::vm::gc::gc_allocate(crate::vm::gc::GcData::Promise(prom));

    // 2. Take the stack and frames to suspend the VM
    let suspended_stack = std::mem::take(&mut vm.stack);
    let suspended_frames = std::mem::take(&mut vm.frames);

    unsafe {
        match &mut (*promise_ptr).data {
            crate::vm::gc::GcData::Promise(p) => {
                *p.suspended_stack.lock().unwrap() = suspended_stack;
                *p.suspended_frames.lock().unwrap() = suspended_frames;
            }
            _ => unreachable!(),
        }
    }

    // 3. Increment active async tasks counter
    let active_counter = vm.active_async_tasks.clone();
    let queue = vm.event_loop_queue.clone();
    active_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    let promise_ptr_usize = promise_ptr as usize;

    // 4. Spawn background thread to fetch URL
    std::thread::spawn(move || {
        let promise_ptr = promise_ptr_usize as *mut crate::vm::gc::GcObject;
        let agent = get_http_agent();
        let res = match agent.get(&url_str).call() {
            Ok(resp) => resp.into_string().map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        };

        // Post ResolveFetchPromise back to event loop
        let mut q = queue.lock().unwrap();
        q.push(crate::vm::execute::EventLoopTask {
            callback: Value::null(),
            args: Vec::new(),
            result: crate::vm::execute::AsyncResult::ResolveFetchPromise(promise_ptr, res),
        });

        active_counter.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    });

    Value::null()
}


pub fn native_future_await(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let future_val = args[0];
    let promise_ptr = if future_val.is_promise() {
        future_val.as_gc_ptr()
    } else if future_val.is_object() {
        unsafe {
            match &(*future_val.as_gc_ptr()).data {
                GcData::Object(map) => {
                    let key = crate::vm::gc::get_or_create_string("_promise");
                    if let Some(val) = map.get(&crate::vm::value::MapKey(Value::string(key))) {
                        if val.is_promise() {
                            val.as_gc_ptr()
                        } else {
                            return Value::null();
                        }
                    } else {
                        return Value::null();
                    }
                }
                GcData::Struct(s) => {
                    if let Some(val) = s.get_field_by_name("_promise") {
                        if val.is_promise() {
                            val.as_gc_ptr()
                        } else {
                            return Value::null();
                        }
                    } else {
                        return Value::null();
                    }
                }
                _ => return Value::null(),
            }
        }
    } else {
        return Value::null();
    };

    unsafe {
        match &(*promise_ptr).data {
            GcData::Promise(prom) => {
                let state = prom.state.lock().unwrap();
                match &*state {
                    crate::vm::gc::PromiseState::Fulfilled(val) => {
                        return *val;
                    }
                    crate::vm::gc::PromiseState::Rejected(err) => {
                        eprintln!("[Future Await] Error: {}", err);
                        return Value::null();
                    }
                    crate::vm::gc::PromiseState::Pending => {}
                }
            }
            _ => return Value::null(),
        }
    }

    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() {
        return Value::null();
    }
    let vm = unsafe { &mut *vm_ptr };

    let suspended_stack = std::mem::take(&mut vm.stack);
    let suspended_frames = std::mem::take(&mut vm.frames);

    unsafe {
        match &mut (*promise_ptr).data {
            GcData::Promise(p) => {
                *p.suspended_stack.lock().unwrap() = suspended_stack;
                *p.suspended_frames.lock().unwrap() = suspended_frames;
            }
            _ => unreachable!(),
        }
    }

    Value::null()
}

pub fn native_set_io_mode(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let mode_val = args[0];
    if !mode_val.is_string() {
        return Value::null();
    }
    let mode_str = unsafe {
        match &(*mode_val.as_gc_ptr()).data {
            GcData::String(s) => s.as_ref().to_string(),
            _ => return Value::null(),
        }
    };
    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if !vm_ptr.is_null() {
        let vm = unsafe { &mut *vm_ptr };
        if mode_str == "evented" {
            vm.use_evented_io = true;
        } else {
            vm.use_evented_io = false;
        }
    }
    Value::null()
}

pub fn native_get_io_mode(_args: Vec<Value>) -> Value {
    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if !vm_ptr.is_null() {
        let vm = unsafe { &*vm_ptr };
        let mode = if vm.use_evented_io { "evented" } else { "threaded" };
        let ptr = crate::vm::gc::gc_allocate(crate::vm::gc::GcData::String(std::rc::Rc::from(mode)));
        Value::string(ptr)
    } else {
        Value::null()
    }
}

pub fn native_array_len(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::number(0.0);
    }
    let arr_val = args[0];
    if !arr_val.is_array() {
        return Value::number(0.0);
    }
    let arr_ptr = arr_val.as_gc_ptr();
    unsafe {
        match &(*arr_ptr).data {
            GcData::Array(arr) => Value::number(arr.len() as f64),
            _ => Value::number(0.0),
        }
    }
}

pub fn native_sleep(args: Vec<Value>) -> Value {
    let delay_ms = if args.is_empty() {
        0
    } else {
        args[0].as_number() as u64
    };

    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() {
        return Value::null();
    }
    let vm = unsafe { &mut *vm_ptr };

    let prom = crate::vm::gc::GcPromise {
        state: std::sync::Arc::new(std::sync::Mutex::new(crate::vm::gc::PromiseState::Pending)),
        suspended_stack: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        suspended_frames: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
    };
    let promise_ptr = crate::vm::gc::gc_allocate(crate::vm::gc::GcData::Promise(prom));
    let promise_val = Value::promise(promise_ptr);

    let queue = vm.event_loop_queue.clone();
    let active_counter = vm.active_async_tasks.clone();
    let promise_usize = promise_ptr as usize;

    active_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));

        let promise_ptr = promise_usize as *mut crate::vm::gc::GcObject;
        let mut q = queue.lock().unwrap();
        q.push(crate::vm::execute::EventLoopTask {
            callback: Value::null(),
            args: Vec::new(),
            result: crate::vm::execute::AsyncResult::ResolvePromise(promise_ptr, Value::null()),
        });

        active_counter.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    });

    promise_val
}
pub fn native_create_promise_pair(_args: Vec<Value>) -> Value {
    let prom = crate::vm::gc::GcPromise {
        state: std::sync::Arc::new(std::sync::Mutex::new(crate::vm::gc::PromiseState::Pending)),
        suspended_stack: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        suspended_frames: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
    };
    let promise_ptr = crate::vm::gc::gc_allocate(crate::vm::gc::GcData::Promise(prom));
    let promise_val = Value::promise(promise_ptr);

    let resolver = Value(crate::vm::value::TAG_METHOD_RESOLVE | (promise_ptr as u64 & crate::vm::value::PTR_MASK));

    let mut map = crate::vm::gc::get_pooled_map(2);
    let promise_key = crate::vm::gc::get_or_create_string("promise");
    let resolve_key = crate::vm::gc::get_or_create_string("resolve");
    map.insert(crate::vm::value::MapKey(Value::string(promise_key)), promise_val);
    map.insert(crate::vm::value::MapKey(Value::string(resolve_key)), resolver);

    let ptr = crate::vm::gc::gc_allocate(crate::vm::gc::GcData::Object(map));
    Value::object(ptr)
}

pub fn native_eronom_is_file(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::boolean(false);
    }
    let val = args[0];
    if val.is_object() {
        let ptr = val.as_gc_ptr();
        unsafe {
            match &(*ptr).data {
                crate::vm::gc::GcData::Struct(s) => {
                    return Value::boolean(s.descriptor.name.as_ref() == "File");
                }
                _ => {}
            }
        }
    }
    Value::boolean(false)
}

pub fn native_eronom_get_mime_type(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let path_val = args[0];
    let path = match path_val.as_str() {
        Some(s) => s,
        None => return Value::null(),
    };
    
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
        
    let mime = match ext.as_str() {
        "json" => "application/json;charset=utf-8",
        "html" | "htm" => "text/html;charset=utf-8",
        "js" | "mjs" => "text/javascript;charset=utf-8",
        "css" => "text/css;charset=utf-8",
        "txt" => "text/plain;charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        _ => "text/plain;charset=utf-8",
    };
    
    let ptr = crate::vm::gc::get_or_create_string(mime);
    Value::string(ptr)
}

pub fn native_eronom_get_file_size(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::number(0.0);
    }
    let path_val = args[0];
    let path = match path_val.as_str() {
        Some(s) => s,
        None => return Value::number(0.0),
    };
    
    let size = std::fs::metadata(path)
        .map(|m| m.len())
        .unwrap_or(0);
        
    Value::number(size as f64)
}

pub fn native_eronom_file_exists(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::boolean(false);
    }
    let path_val = args[0];
    let path_str = match path_val.as_str() {
        Some(s) => s.to_string(),
        None => return Value::boolean(false),
    };

    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() {
        return Value::boolean(std::path::Path::new(&path_str).exists());
    }
    let vm = unsafe { &mut *vm_ptr };

    if vm.use_evented_io {
        let state = std::sync::Arc::new(std::sync::Mutex::new(crate::vm::gc::PromiseState::Pending));
        let prom = crate::vm::gc::GcPromise {
            state: state.clone(),
            suspended_stack: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            suspended_frames: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        };
        let promise_ptr = crate::vm::gc::gc_allocate(crate::vm::gc::GcData::Promise(prom));

        let suspended_stack = std::mem::take(&mut vm.stack);
        let suspended_frames = std::mem::take(&mut vm.frames);

        unsafe {
            match &mut (*promise_ptr).data {
                crate::vm::gc::GcData::Promise(p) => {
                    *p.suspended_stack.lock().unwrap() = suspended_stack;
                    *p.suspended_frames.lock().unwrap() = suspended_frames;
                }
                _ => unreachable!(),
            }
        }

        let active_counter = vm.active_async_tasks.clone();
        let queue = vm.event_loop_queue.clone();
        active_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        let promise_ptr_usize = promise_ptr as usize;

        std::thread::spawn(move || {
            let promise_ptr = promise_ptr_usize as *mut crate::vm::gc::GcObject;
            let exists = std::path::Path::new(&path_str).exists();
            let res_val = Value::boolean(exists);

            let mut q = queue.lock().unwrap();
            q.push(crate::vm::execute::EventLoopTask {
                callback: Value::null(),
                args: Vec::new(),
                result: crate::vm::execute::AsyncResult::ResolvePromise(promise_ptr, res_val),
            });

            active_counter.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        });

        Value::null()
    } else {
        Value::boolean(std::path::Path::new(&path_str).exists())
    }
}

pub fn native_eronom_file_text(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let path_val = args[0];
    let path_str = match path_val.as_str() {
        Some(s) => s.to_string(),
        None => return Value::null(),
    };

    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() {
        let content = std::fs::read_to_string(&path_str).unwrap_or_default();
        let ptr = crate::vm::gc::get_or_create_string(&content);
        return Value::string(ptr);
    }
    let vm = unsafe { &mut *vm_ptr };

    if vm.use_evented_io {
        let state = std::sync::Arc::new(std::sync::Mutex::new(crate::vm::gc::PromiseState::Pending));
        let prom = crate::vm::gc::GcPromise {
            state: state.clone(),
            suspended_stack: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            suspended_frames: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        };
        let promise_ptr = crate::vm::gc::gc_allocate(crate::vm::gc::GcData::Promise(prom));

        let suspended_stack = std::mem::take(&mut vm.stack);
        let suspended_frames = std::mem::take(&mut vm.frames);

        unsafe {
            match &mut (*promise_ptr).data {
                crate::vm::gc::GcData::Promise(p) => {
                    *p.suspended_stack.lock().unwrap() = suspended_stack;
                    *p.suspended_frames.lock().unwrap() = suspended_frames;
                }
                _ => unreachable!(),
            }
        }

        let active_counter = vm.active_async_tasks.clone();
        let queue = vm.event_loop_queue.clone();
        active_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        let promise_ptr_usize = promise_ptr as usize;

        std::thread::spawn(move || {
            let promise_ptr = promise_ptr_usize as *mut crate::vm::gc::GcObject;
            let content = std::fs::read_to_string(&path_str).map_err(|e| e.to_string());

            let mut q = queue.lock().unwrap();
            q.push(crate::vm::execute::EventLoopTask {
                callback: Value::null(),
                args: Vec::new(),
                result: crate::vm::execute::AsyncResult::ResolveTextPromise(promise_ptr, content),
            });

            active_counter.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        });

        Value::null()
    } else {
        let content = std::fs::read_to_string(&path_str).unwrap_or_default();
        let ptr = crate::vm::gc::get_or_create_string(&content);
        Value::string(ptr)
    }
}

pub fn native_eronom_file_json(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let path_val = args[0];
    let path_str = match path_val.as_str() {
        Some(s) => s.to_string(),
        None => return Value::null(),
    };

    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() {
        let content = std::fs::read_to_string(&path_str).unwrap_or_default();
        if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&content) {
            return crate::vm::gc::json_to_value(json_val);
        } else {
            return Value::null();
        }
    }
    let vm = unsafe { &mut *vm_ptr };

    if vm.use_evented_io {
        let state = std::sync::Arc::new(std::sync::Mutex::new(crate::vm::gc::PromiseState::Pending));
        let prom = crate::vm::gc::GcPromise {
            state: state.clone(),
            suspended_stack: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            suspended_frames: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        };
        let promise_ptr = crate::vm::gc::gc_allocate(crate::vm::gc::GcData::Promise(prom));

        let suspended_stack = std::mem::take(&mut vm.stack);
        let suspended_frames = std::mem::take(&mut vm.frames);

        unsafe {
            match &mut (*promise_ptr).data {
                crate::vm::gc::GcData::Promise(p) => {
                    *p.suspended_stack.lock().unwrap() = suspended_stack;
                    *p.suspended_frames.lock().unwrap() = suspended_frames;
                }
                _ => unreachable!(),
            }
        }

        let active_counter = vm.active_async_tasks.clone();
        let queue = vm.event_loop_queue.clone();
        active_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        let promise_ptr_usize = promise_ptr as usize;

        std::thread::spawn(move || {
            let promise_ptr = promise_ptr_usize as *mut crate::vm::gc::GcObject;
            let content = std::fs::read_to_string(&path_str).map_err(|e| e.to_string());

            let mut q = queue.lock().unwrap();
            q.push(crate::vm::execute::EventLoopTask {
                callback: Value::null(),
                args: Vec::new(),
                result: crate::vm::execute::AsyncResult::ResolveJsonPromise(promise_ptr, content),
            });

            active_counter.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        });

        Value::null()
    } else {
        let content = std::fs::read_to_string(&path_str).unwrap_or_default();
        if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&content) {
            crate::vm::gc::json_to_value(json_val)
        } else {
            Value::null()
        }
    }
}

pub fn native_eronom_write_file(args: Vec<Value>) -> Value {
    if args.len() < 3 {
        return Value::number(0.0);
    }
    let path_val = args[0];
    let data_val = args[1];
    let is_src_file = args[2].as_boolean();

    let path_str = match path_val.as_str() {
        Some(s) => s.to_string(),
        None => return Value::number(0.0),
    };

    let data_str = match data_val.as_str() {
        Some(s) => s.to_string(),
        None => return Value::number(0.0),
    };

    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() {
        let res = write_file_helper(&path_str, &data_str, is_src_file);
        return Value::number(res.unwrap_or(0) as f64);
    }
    let vm = unsafe { &mut *vm_ptr };

    if vm.use_evented_io {
        let state = std::sync::Arc::new(std::sync::Mutex::new(crate::vm::gc::PromiseState::Pending));
        let prom = crate::vm::gc::GcPromise {
            state: state.clone(),
            suspended_stack: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            suspended_frames: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        };
        let promise_ptr = crate::vm::gc::gc_allocate(crate::vm::gc::GcData::Promise(prom));

        let suspended_stack = std::mem::take(&mut vm.stack);
        let suspended_frames = std::mem::take(&mut vm.frames);

        unsafe {
            match &mut (*promise_ptr).data {
                crate::vm::gc::GcData::Promise(p) => {
                    *p.suspended_stack.lock().unwrap() = suspended_stack;
                    *p.suspended_frames.lock().unwrap() = suspended_frames;
                }
                _ => unreachable!(),
            }
        }

        let active_counter = vm.active_async_tasks.clone();
        let queue = vm.event_loop_queue.clone();
        active_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        let promise_ptr_usize = promise_ptr as usize;

        std::thread::spawn(move || {
            let promise_ptr = promise_ptr_usize as *mut crate::vm::gc::GcObject;
            let res = write_file_helper(&path_str, &data_str, is_src_file);

            let mut q = queue.lock().unwrap();
            q.push(crate::vm::execute::EventLoopTask {
                callback: Value::null(),
                args: Vec::new(),
                result: crate::vm::execute::AsyncResult::ResolveWritePromise(promise_ptr, res),
            });

            active_counter.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        });

        Value::null()
    } else {
        let res = write_file_helper(&path_str, &data_str, is_src_file);
        Value::number(res.unwrap_or(0) as f64)
    }
}

fn write_file_helper(path: &str, data: &str, is_src_file: bool) -> Result<usize, String> {
    if is_src_file {
        std::fs::copy(data, path)
            .map(|bytes| bytes as usize)
            .map_err(|e| e.to_string())
    } else {
        std::fs::write(path, data)
            .map(|_| data.len())
            .map_err(|e| e.to_string())
    }
}

pub fn register_eronom_file_api(vm: &mut VM) -> Result<(), String> {
    // 1. Register native functions for Eronom File API
    vm.register_global("Eronom_nativeFileExists", Value::native_function(native_eronom_file_exists));
    vm.register_global("Eronom_nativeFileText", Value::native_function(native_eronom_file_text));
    vm.register_global("Eronom_nativeFileJson", Value::native_function(native_eronom_file_json));
    vm.register_global("Eronom_nativeGetMimeType", Value::native_function(native_eronom_get_mime_type));
    vm.register_global("Eronom_nativeGetFileSize", Value::native_function(native_eronom_get_file_size));
    vm.register_global("Eronom_nativeIsFile", Value::native_function(native_eronom_is_file));
    vm.register_global("Eronom_nativeWriteFile", Value::native_function(native_eronom_write_file));
    Ok(())
}

