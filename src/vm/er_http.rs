use std::cell::Cell;
use std::cell::RefCell;
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

thread_local! {
    pub static ROUTES: RefCell<Vec<Route>> = RefCell::new(Vec::new());
    pub static ACTIVE_VM: Cell<*mut VM> = const { Cell::new(std::ptr::null_mut()) };
    pub static ACTIVE_HTTP_RESPONSE: Cell<*mut c_void> = const { Cell::new(std::ptr::null_mut()) };
    static TARGET_SCRIPT_PATH: RefCell<Option<String>> = const { RefCell::new(None) };
    static LAST_MTIME: Cell<Option<SystemTime>> = const { Cell::new(None) };
}

unsafe extern "C" {
    fn er_http_init();
    fn er_http_register_route(method: *const c_char, path: *const c_char);
    fn er_http_listen_and_run(port: i32);
    fn er_http_response_end_json(res: *mut c_void, json_str: *const c_char, json_len: usize);
}

pub fn native_route(_args: Vec<Value>) -> Value {
    let router_obj = crate::vm::gc::get_pooled_map(2);
    
    let get_name = get_or_create_string("get");
    let post_name = get_or_create_string("post");
    
    let get_fn = Value::native_function(native_router_get);
    let post_fn = Value::native_function(native_router_post);
    
    let mut map = router_obj;
    map.insert(crate::vm::value::MapKey(Value::string(get_name)), get_fn);
    map.insert(crate::vm::value::MapKey(Value::string(post_name)), post_fn);
    
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
    let has_routes = ROUTES.with(|r| !r.borrow().is_empty());
    if !has_routes {
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
    
    crate::vm::gc::GC_ROOTS.with(|roots| {
        roots.borrow_mut().push(Box::new(|| {
            ROUTES.with(|routes| {
                for route in routes.borrow().iter() {
                    crate::vm::gc::mark_value(&route.callback);
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
    
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[HTTP] Reload error: Failed to read file: {}", e);
            return;
        }
    };
    
    let old_routes = ROUTES.with(|r| std::mem::take(&mut *r.borrow_mut()));
    
    let tokens = crate::frontend::lex(&content);
    let mut parser = crate::frontend::Parser::new(tokens);
    let stmts = match parser.parse() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[HTTP] Reload error: Parsing failed: {}", e);
            ROUTES.with(|r| *r.borrow_mut() = old_routes);
            return;
        }
    };
    
    let compiler = crate::vm::compiler::Compiler::new();
    let function = match compiler.compile(&stmts) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[HTTP] Reload error: Compilation failed: {}", e);
            ROUTES.with(|r| *r.borrow_mut() = old_routes);
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
                
                let context_obj = crate::vm::gc::get_pooled_map(1);
                let json_name = get_or_create_string("json");
                let json_fn = Value::native_function(native_context_json);
                let mut map = context_obj;
                map.insert(crate::vm::value::MapKey(Value::string(json_name)), json_fn);
                let c_val = Value::object(crate::vm::gc::gc_allocate(GcData::Object(map)));
                
                if let Err(e) = vm.call_function_reentrant(callback, vec![c_val]) {
                    eprintln!("[HTTP] Error executing callback: {}", e);
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
