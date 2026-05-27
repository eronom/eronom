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
    static TARGET_SCRIPT_PATH: RefCell<Option<String>> = const { RefCell::new(None) };
    static LAST_MTIME: Cell<Option<SystemTime>> = const { Cell::new(None) };
    static LAST_CHECK_TIME: Cell<Option<SystemTime>> = const { Cell::new(None) };
}

unsafe extern "C" {
    fn er_http_init();
    fn er_http_register_route(method: *const c_char, path: *const c_char);
    fn er_http_listen_and_run(port: i32);
    fn er_http_response_end_json(res: *mut c_void, json_str: *const c_char, json_len: usize);
    
    fn er_ws_register_route(path: *const c_char);
    fn er_ws_send(ws: *mut c_void, message: *const c_char, message_len: usize);
    fn er_ws_close(ws: *mut c_void);
}

pub fn native_route(_args: Vec<Value>) -> Value {
    let router_obj = crate::vm::gc::get_pooled_map(4);
    
    let get_name = get_or_create_string("get");
    let post_name = get_or_create_string("post");
    let ws_name = get_or_create_string("ws");
    let use_name = get_or_create_string("use");
    
    let get_fn = Value::native_function(native_router_get);
    let post_fn = Value::native_function(native_router_post);
    let ws_fn = Value::native_function(native_router_ws);
    let use_fn = Value::native_function(native_router_use);
    
    let mut map = router_obj;
    map.insert(crate::vm::value::MapKey(Value::string(get_name)), get_fn);
    map.insert(crate::vm::value::MapKey(Value::string(post_name)), post_fn);
    map.insert(crate::vm::value::MapKey(Value::string(ws_name)), ws_fn);
    map.insert(crate::vm::value::MapKey(Value::string(use_name)), use_fn);
    
    let ptr = gc_allocate(GcData::Object(map));
    Value::object(ptr)
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
    
    let path_str = unsafe {
        match &(*path_val.as_gc_ptr()).data {
            GcData::String(s) => s.as_ref().to_string(),
            _ => return Value::null(),
        }
    };
    
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

fn value_to_json(val: Value) -> serde_json::Value {
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
    if !has_http_routes && !has_ws_routes {
        return;
    }
    
    let port = get_port_from_config(vm);
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
        }));
    });
    
    ACTIVE_VM.with(|active| {
        active.set(vm as *mut VM);
    });
    
    unsafe {
        er_http_listen_and_run(port);
    }
    
    ACTIVE_VM.with(|active| {
        active.set(std::ptr::null_mut());
    });
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
    
    let path_buf = Path::new(&path);
    let stmts = match crate::frontend::parse_and_resolve_imports(path_buf) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[HTTP] Reload error: Parsing/Import resolution failed: {}", e);
            ROUTES.with(|r| *r.borrow_mut() = old_routes);
            WS_ROUTES.with(|r| *r.borrow_mut() = old_ws_routes);
            MIDDLEWARES.with(|r| *r.borrow_mut() = old_mws);
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
        return;
    }
    
    LAST_MTIME.with(|m| m.set(Some(current_mtime)));
    println!("[HTTP] Reload successful. VM state and routes updated.");
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
    
    let callback_opt = ROUTES.with(|routes| {
        for route in routes.borrow().iter() {
            if route.method == method && route.path == path {
                return Some(route.callback);
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
                
                let mut req_map = crate::vm::gc::get_pooled_map(2);
                let url_name = get_or_create_string("url");
                let method_name = get_or_create_string("method");
                let path_str = get_or_create_string(path);
                let method_str = get_or_create_string(method);
                
                req_map.insert(crate::vm::value::MapKey(Value::string(url_name)), Value::string(path_str));
                req_map.insert(crate::vm::value::MapKey(Value::string(method_name)), Value::string(method_str));
                
                let req_obj = Value::object(crate::vm::gc::gc_allocate(GcData::Object(req_map)));
                
                let context_obj = crate::vm::gc::get_pooled_map(2);
                let json_name = get_or_create_string("json");
                let json_fn = Value::native_function(native_context_json);
                let req_key_name = get_or_create_string("req");
                
                let mut map = context_obj;
                map.insert(crate::vm::value::MapKey(Value::string(json_name)), json_fn);
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
