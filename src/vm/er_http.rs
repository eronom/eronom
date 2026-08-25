use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use crate::vm::value::Value;
use crate::vm::execute::VM;
use crate::vm::gc::{get_or_create_string, gc_allocate, GcData};
use crate::vm::router::RadixRouter;
use std::ffi::{c_char, c_void, CString};
use std::time::SystemTime;
use std::fs;
use std::path::Path;

#[derive(Clone)]
pub struct Route {
    pub method: String,
    pub path: String,
    pub callback: Value,
}

#[derive(Clone)]
pub struct WsRoute {
    pub path: String,
    pub open: Option<Value>,
    pub message: Option<Value>,
    pub close: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct ResponseState {
    pub status: Option<u16>,
    pub headers: Vec<(String, String)>,
    pub cookies: Vec<String>,
    pub finished: bool,
}

impl ResponseState {
    pub fn new() -> Self {
        Self {
            status: None,
            headers: Vec::new(),
            cookies: Vec::new(),
            finished: false,
        }
    }

    pub fn reset(&mut self) {
        self.status = None;
        self.headers.clear();
        self.cookies.clear();
        self.finished = false;
    }
}

thread_local! {
    pub static ROUTER: RefCell<RadixRouter> = RefCell::new(RadixRouter::new());
    pub static ROUTES: RefCell<Vec<Route>> = RefCell::new(Vec::new());
    pub static WS_ROUTES: RefCell<Vec<WsRoute>> = RefCell::new(Vec::new());
    pub static STATIC_MOUNTS: RefCell<Vec<(String, String)>> = RefCell::new(Vec::new());
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

    pub static ACTIVE_REQUEST_HEADERS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    pub static ACTIVE_REQUEST_COOKIES: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    pub static ACTIVE_REQUEST_QUERY: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    pub static ACTIVE_REQUEST_PATH: RefCell<String> = RefCell::new(String::new());
    pub static ACTIVE_RESPONSE_STATE: RefCell<ResponseState> = RefCell::new(ResponseState::new());
}

#[allow(dead_code)]
unsafe extern "C" {
    fn er_http_init();
    fn er_http_register_route(method: *const c_char, path: *const c_char);
    fn er_http_listen_and_run(port: i32);
    fn er_http_response_end_json(res: *mut c_void, json_str: *const c_char, json_len: usize) -> bool;
    fn er_http_response_end_html(res: *mut c_void, html_str: *const c_char, html_len: usize) -> bool;
    fn er_http_response_write_status(res: *mut c_void, status_str: *const c_char, status_len: usize) -> bool;
    fn er_http_response_write_header(res: *mut c_void, key_str: *const c_char, key_len: usize, val_str: *const c_char, val_len: usize) -> bool;
    fn er_http_response_end(res: *mut c_void, data_str: *const c_char, data_len: usize) -> bool;
    fn er_http_response_is_alive(res: *mut c_void) -> bool;
    fn er_http_response_release(res: *mut c_void);
    
    fn er_ws_register_route(path: *const c_char);
    fn er_ws_send(ws: *mut c_void, message: *const c_char, message_len: usize, is_binary: i32);
    fn er_ws_close(ws: *mut c_void);
    fn er_ws_close_with_code(ws: *mut c_void, code: i32, message: *const c_char, message_len: usize);
    fn er_ws_subscribe(ws: *mut c_void, topic: *const c_char, topic_len: usize) -> bool;
    fn er_ws_unsubscribe(ws: *mut c_void, topic: *const c_char, topic_len: usize) -> bool;
    fn er_ws_is_subscribed(ws: *mut c_void, topic: *const c_char, topic_len: usize) -> bool;
    fn er_ws_publish(ws: *mut c_void, topic: *const c_char, topic_len: usize, message: *const c_char, message_len: usize, is_binary: i32) -> bool;
    fn er_app_publish(topic: *const c_char, topic_len: usize, message: *const c_char, message_len: usize, is_binary: i32) -> bool;
    fn er_app_num_subscribers(topic: *const c_char, topic_len: usize) -> u32;

    fn er_http_create_timer(ms: i32, cb: extern "C" fn(*mut c_void));
}

pub fn native_route(_args: Vec<Value>) -> Value {
    let router_obj = crate::vm::gc::get_pooled_map(18);
    
    let get_name = get_or_create_string("get");
    let post_name = get_or_create_string("post");
    let put_name = get_or_create_string("put");
    let delete_name = get_or_create_string("delete");
    let del_name = get_or_create_string("del");
    let patch_name = get_or_create_string("patch");
    let head_name = get_or_create_string("head");
    let options_name = get_or_create_string("options");
    let all_name = get_or_create_string("all");
    let any_name = get_or_create_string("any");
    let ws_name = get_or_create_string("ws");
    let use_name = get_or_create_string("use");
    let listen_name = get_or_create_string("listen");
    let static_name = get_or_create_string("static");
    let serve_static_name = get_or_create_string("serveStatic");
    let publish_name = get_or_create_string("publish");
    let num_subscribers_name = get_or_create_string("numSubscribers");
    
    let get_fn = Value::native_function(native_router_get);
    let post_fn = Value::native_function(native_router_post);
    let put_fn = Value::native_function(native_router_put);
    let delete_fn = Value::native_function(native_router_delete);
    let del_fn = Value::native_function(native_router_delete);
    let patch_fn = Value::native_function(native_router_patch);
    let head_fn = Value::native_function(native_router_head);
    let options_fn = Value::native_function(native_router_options);
    let all_fn = Value::native_function(native_router_all);
    let any_fn = Value::native_function(native_router_all);
    let ws_fn = Value::native_function(native_router_ws);
    let use_fn = Value::native_function(native_router_use);
    let listen_fn = Value::native_function(native_router_listen);
    let static_fn = Value::native_function(native_router_static);
    let serve_static_fn = Value::native_function(native_router_static);
    let publish_fn = Value::native_function(native_router_publish);
    let num_subscribers_fn = Value::native_function(native_router_num_subscribers);
    
    let mut map = router_obj;
    map.insert(crate::vm::value::MapKey(Value::string(get_name)), get_fn);
    map.insert(crate::vm::value::MapKey(Value::string(post_name)), post_fn);
    map.insert(crate::vm::value::MapKey(Value::string(put_name)), put_fn);
    map.insert(crate::vm::value::MapKey(Value::string(delete_name)), delete_fn);
    map.insert(crate::vm::value::MapKey(Value::string(del_name)), del_fn);
    map.insert(crate::vm::value::MapKey(Value::string(patch_name)), patch_fn);
    map.insert(crate::vm::value::MapKey(Value::string(head_name)), head_fn);
    map.insert(crate::vm::value::MapKey(Value::string(options_name)), options_fn);
    map.insert(crate::vm::value::MapKey(Value::string(all_name)), all_fn);
    map.insert(crate::vm::value::MapKey(Value::string(any_name)), any_fn);
    map.insert(crate::vm::value::MapKey(Value::string(ws_name)), ws_fn);
    map.insert(crate::vm::value::MapKey(Value::string(use_name)), use_fn);
    map.insert(crate::vm::value::MapKey(Value::string(listen_name)), listen_fn);
    map.insert(crate::vm::value::MapKey(Value::string(static_name)), static_fn);
    map.insert(crate::vm::value::MapKey(Value::string(serve_static_name)), serve_static_fn);
    map.insert(crate::vm::value::MapKey(Value::string(publish_name)), publish_fn);
    map.insert(crate::vm::value::MapKey(Value::string(num_subscribers_name)), num_subscribers_fn);
    
    let ptr = gc_allocate(GcData::Object(map));
    Value::object(ptr)
}

pub fn native_router_publish(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::boolean(false);
    }
    let topic_str = match args[0].as_str() {
        Some(s) => s,
        None => return Value::boolean(false),
    };
    let is_binary_arg = if args.len() > 2 {
        args[2].as_boolean() || (args[2].is_number() && args[2].as_number() != 0.0)
    } else {
        false
    };
    let (bytes, is_binary) = extract_bytes_from_value(args[1], is_binary_arg);
    let result = unsafe {
        er_app_publish(
            topic_str.as_ptr() as *const c_char,
            topic_str.len(),
            bytes.as_ptr() as *const c_char,
            bytes.len(),
            if is_binary { 1 } else { 0 },
        )
    };
    Value::boolean(result)
}

pub fn native_router_num_subscribers(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::number(0.0);
    }
    let topic_str = match args[0].as_str() {
        Some(s) => s,
        None => return Value::number(0.0),
    };
    let count = unsafe {
        er_app_num_subscribers(topic_str.as_ptr() as *const c_char, topic_str.len())
    };
    Value::number(count as f64)
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

pub fn extract_bytes_from_value(val: Value, force_binary: bool) -> (Vec<u8>, bool) {
    if val.is_array() {
        let ptr = val.as_gc_ptr();
        let bytes = unsafe {
            match &(*ptr).data {
                GcData::Array(arr) => {
                    arr.iter().map(|v| {
                        if v.is_number() {
                            v.as_number() as u8
                        } else {
                            0
                        }
                    }).collect()
                }
                _ => Vec::new(),
            }
        };
        (bytes, true)
    } else if val.is_string() {
        let s = unsafe {
            match &(*val.as_gc_ptr()).data {
                GcData::String(s) => s.as_bytes().to_vec(),
                _ => Vec::new(),
            }
        };
        (s, force_binary)
    } else {
        let s = val.to_string();
        (s.into_bytes(), force_binary)
    }
}

pub fn native_ws_send(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let message_val = args[0];
    let is_binary_arg = if args.len() > 1 {
        args[1].as_boolean() || (args[1].is_number() && args[1].as_number() != 0.0)
    } else {
        false
    };
    
    let (bytes, is_binary) = extract_bytes_from_value(message_val, is_binary_arg);
    
    ACTIVE_WEBSOCKET.with(|active| {
        let ptr = active.get();
        if !ptr.is_null() {
            unsafe {
                er_ws_send(ptr, bytes.as_ptr() as *const c_char, bytes.len(), if is_binary { 1 } else { 0 });
            }
        } else {
            eprintln!("[WS] Error: ACTIVE_WEBSOCKET is null when calling send()");
        }
    });
    
    Value::null()
}

pub fn native_ws_close(args: Vec<Value>) -> Value {
    let mut code = 0i32;
    let mut reason = String::new();
    if !args.is_empty() && args[0].is_number() {
        code = args[0].as_number() as i32;
    }
    if args.len() > 1 {
        if let Some(s) = args[1].as_str() {
            reason = s.to_string();
        }
    }
    ACTIVE_WEBSOCKET.with(|active| {
        let ptr = active.get();
        if !ptr.is_null() {
            unsafe {
                if code > 0 || !reason.is_empty() {
                    let c_str = CString::new(reason).unwrap();
                    er_ws_close_with_code(ptr, code, c_str.as_ptr(), c_str.as_bytes().len());
                } else {
                    er_ws_close(ptr);
                }
            }
        } else {
            eprintln!("[WS] Error: ACTIVE_WEBSOCKET is null when calling close()");
        }
    });
    Value::null()
}

pub fn native_ws_subscribe(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::boolean(false);
    }
    let topic_str = match args[0].as_str() {
        Some(s) => s,
        None => return Value::boolean(false),
    };
    let mut result = false;
    ACTIVE_WEBSOCKET.with(|active| {
        let ptr = active.get();
        if !ptr.is_null() {
            unsafe {
                result = er_ws_subscribe(ptr, topic_str.as_ptr() as *const c_char, topic_str.len());
            }
        } else {
            eprintln!("[WS] Error: ACTIVE_WEBSOCKET is null when calling ws.subscribe()");
        }
    });
    Value::boolean(result)
}

pub fn native_ws_unsubscribe(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::boolean(false);
    }
    let topic_str = match args[0].as_str() {
        Some(s) => s,
        None => return Value::boolean(false),
    };
    let mut result = false;
    ACTIVE_WEBSOCKET.with(|active| {
        let ptr = active.get();
        if !ptr.is_null() {
            unsafe {
                result = er_ws_unsubscribe(ptr, topic_str.as_ptr() as *const c_char, topic_str.len());
            }
        } else {
            eprintln!("[WS] Error: ACTIVE_WEBSOCKET is null when calling ws.unsubscribe()");
        }
    });
    Value::boolean(result)
}

pub fn native_ws_is_subscribed(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::boolean(false);
    }
    let topic_str = match args[0].as_str() {
        Some(s) => s,
        None => return Value::boolean(false),
    };
    let mut result = false;
    ACTIVE_WEBSOCKET.with(|active| {
        let ptr = active.get();
        if !ptr.is_null() {
            unsafe {
                result = er_ws_is_subscribed(ptr, topic_str.as_ptr() as *const c_char, topic_str.len());
            }
        } else {
            eprintln!("[WS] Error: ACTIVE_WEBSOCKET is null when calling ws.isSubscribed()");
        }
    });
    Value::boolean(result)
}

pub fn native_ws_publish(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::boolean(false);
    }
    let topic_str = match args[0].as_str() {
        Some(s) => s,
        None => return Value::boolean(false),
    };
    let is_binary_arg = if args.len() > 2 {
        args[2].as_boolean() || (args[2].is_number() && args[2].as_number() != 0.0)
    } else {
        false
    };
    let (bytes, is_binary) = extract_bytes_from_value(args[1], is_binary_arg);
    let mut result = false;
    ACTIVE_WEBSOCKET.with(|active| {
        let ptr = active.get();
        if !ptr.is_null() {
            unsafe {
                result = er_ws_publish(
                    ptr,
                    topic_str.as_ptr() as *const c_char,
                    topic_str.len(),
                    bytes.as_ptr() as *const c_char,
                    bytes.len(),
                    if is_binary { 1 } else { 0 },
                );
            }
        } else {
            eprintln!("[WS] Error: ACTIVE_WEBSOCKET is null when calling ws.publish()");
        }
    });
    Value::boolean(result)
}

fn create_ws_object(_ws: *mut c_void) -> Value {
    let ws_map = crate::vm::gc::get_pooled_map(8);
    
    let send_name = get_or_create_string("send");
    let send_fn = Value::native_function(native_ws_send);
    
    let close_name = get_or_create_string("close");
    let close_fn = Value::native_function(native_ws_close);

    let subscribe_name = get_or_create_string("subscribe");
    let subscribe_fn = Value::native_function(native_ws_subscribe);

    let unsubscribe_name = get_or_create_string("unsubscribe");
    let unsubscribe_fn = Value::native_function(native_ws_unsubscribe);

    let is_subscribed_name = get_or_create_string("isSubscribed");
    let is_subscribed_fn = Value::native_function(native_ws_is_subscribed);

    let publish_name = get_or_create_string("publish");
    let publish_fn = Value::native_function(native_ws_publish);
    
    let mut map = ws_map;
    map.insert(crate::vm::value::MapKey(Value::string(send_name)), send_fn);
    map.insert(crate::vm::value::MapKey(Value::string(close_name)), close_fn);
    map.insert(crate::vm::value::MapKey(Value::string(subscribe_name)), subscribe_fn);
    map.insert(crate::vm::value::MapKey(Value::string(unsubscribe_name)), unsubscribe_fn);
    map.insert(crate::vm::value::MapKey(Value::string(is_subscribed_name)), is_subscribed_fn);
    map.insert(crate::vm::value::MapKey(Value::string(publish_name)), publish_fn);
    
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
    if !callback_val.is_function() && !callback_val.is_native_function() {
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

    ROUTER.with(|r| {
        r.borrow_mut().insert(method, &path_str, callback_val);
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

pub fn native_router_put(args: Vec<Value>) -> Value {
    register_route_internal("PUT", args)
}

pub fn native_router_delete(args: Vec<Value>) -> Value {
    register_route_internal("DELETE", args)
}

pub fn native_router_patch(args: Vec<Value>) -> Value {
    register_route_internal("PATCH", args)
}

pub fn native_router_head(args: Vec<Value>) -> Value {
    register_route_internal("HEAD", args)
}

pub fn native_router_options(args: Vec<Value>) -> Value {
    register_route_internal("OPTIONS", args)
}

pub fn native_router_all(args: Vec<Value>) -> Value {
    register_route_internal("ALL", args)
}

pub fn native_router_static(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        eprintln!("[HTTP] Error: app.static requires prefix and root directory");
        return Value::null();
    }
    let prefix_str = match args[0].as_str() {
        Some(s) => s.to_string(),
        None => return Value::null(),
    };
    let root_dir_str = match args[1].as_str() {
        Some(s) => s.to_string(),
        None => return Value::null(),
    };

    let clean_prefix = if prefix_str.len() > 1 && prefix_str.ends_with('/') {
        &prefix_str[..prefix_str.len() - 1]
    } else {
        &prefix_str
    };

    STATIC_MOUNTS.with(|mounts| {
        mounts.borrow_mut().push((clean_prefix.to_string(), root_dir_str.clone()));
    });

    let pattern_wildcard = if clean_prefix == "/" || clean_prefix.is_empty() {
        "/*filepath".to_string()
    } else {
        format!("{}/*filepath", clean_prefix)
    };

    let static_handler = Value::native_function(native_static_route_handler);

    register_route_internal("GET", vec![Value::string(get_or_create_string(&pattern_wildcard)), static_handler]);
    register_route_internal("HEAD", vec![Value::string(get_or_create_string(&pattern_wildcard)), static_handler]);

    if clean_prefix != "/" && !clean_prefix.is_empty() {
        register_route_internal("GET", vec![Value::string(get_or_create_string(clean_prefix)), static_handler]);
        register_route_internal("HEAD", vec![Value::string(get_or_create_string(clean_prefix)), static_handler]);
    }

    Value::null()
}

pub fn percent_decode(input: &str) -> String {
    let mut bytes = Vec::with_capacity(input.len());
    let input_bytes = input.as_bytes();
    let mut i = 0;
    while i < input_bytes.len() {
        match input_bytes[i] {
            b'+' => {
                bytes.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < input_bytes.len() => {
                let hex_str = std::str::from_utf8(&input_bytes[i+1..i+3]).unwrap_or("");
                if let Ok(byte) = u8::from_str_radix(hex_str, 16) {
                    bytes.push(byte);
                    i += 3;
                } else {
                    bytes.push(b'%');
                    i += 1;
                }
            }
            b => {
                bytes.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

pub fn parse_query_string(raw: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let query_str = if let Some(pos) = raw.find('?') {
        &raw[pos + 1..]
    } else {
        raw
    };
    if query_str.is_empty() {
        return map;
    }
    for pair in query_str.split('&') {
        if pair.is_empty() {
            continue;
        }
        if let Some(pos) = pair.find('=') {
            let k = percent_decode(&pair[..pos]);
            let v = percent_decode(&pair[pos + 1..]);
            map.insert(k, v);
        } else {
            let k = percent_decode(pair);
            map.insert(k, "true".to_string());
        }
    }
    map
}

pub fn parse_cookies(cookie_header: &str) -> HashMap<String, String> {
    let mut cookies = HashMap::new();
    for item in cookie_header.split(';') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        if let Some(pos) = item.find('=') {
            let name = item[..pos].trim();
            let val = percent_decode(item[pos + 1..].trim());
            cookies.insert(name.to_string(), val);
        }
    }
    cookies
}

pub fn format_cookie(name: &str, val: &str, options: Option<Value>) -> String {
    let mut out = format!("{}={}", name, val);
    let mut path_set = false;

    if let Some(opt) = options {
        if opt.is_object() {
            let ptr = opt.as_gc_ptr();
            unsafe {
                if let GcData::Object(map) = &(*ptr).data {
                    for (k, v) in map {
                        if let Some(key_str) = k.0.as_str() {
                            let lower = key_str.to_ascii_lowercase();
                            match lower.as_str() {
                                "path" => {
                                    if let Some(p) = v.as_str() {
                                        out.push_str(&format!("; Path={}", p));
                                        path_set = true;
                                    }
                                }
                                "domain" => {
                                    if let Some(d) = v.as_str() {
                                        out.push_str(&format!("; Domain={}", d));
                                    }
                                }
                                "maxage" | "max_age" => {
                                    if v.is_number() {
                                        out.push_str(&format!("; Max-Age={}", v.as_number() as i64));
                                    }
                                }
                                "expires" => {
                                    if let Some(exp) = v.as_str() {
                                        out.push_str(&format!("; Expires={}", exp));
                                    }
                                }
                                "httponly" | "http_only" => {
                                    if v.as_boolean() {
                                        out.push_str("; HttpOnly");
                                    }
                                }
                                "secure" => {
                                    if v.as_boolean() {
                                        out.push_str("; Secure");
                                    }
                                }
                                "samesite" | "same_site" => {
                                    if let Some(ss) = v.as_str() {
                                        out.push_str(&format!("; SameSite={}", ss));
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }

    if !path_set {
        out.push_str("; Path=/");
    }

    out
}

pub fn get_mime_type_for_extension(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" | "cjs" => "application/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
        "md" => "text/markdown; charset=utf-8",
        "xml" => "application/xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "bmp" => "image/bmp",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "eot" => "application/vnd.ms-fontobject",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        "pdf" => "application/pdf",
        "wasm" => "application/wasm",
        "zip" => "application/zip",
        "tar" => "application/x-tar",
        "gz" => "application/gzip",
        _ => "application/octet-stream",
    }
}

pub fn calculate_etag(size: u64, mtime: SystemTime) -> String {
    let secs = mtime
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("W/\"{:x}-{:x}\"", size, secs)
}

pub fn format_http_date(time: SystemTime) -> String {
    let secs = time
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let days_since_epoch = secs / 86400;
    let time_of_day = secs % 86400;
    let hour = time_of_day / 3600;
    let min = (time_of_day % 3600) / 60;
    let sec = time_of_day % 60;

    let day_of_week = match (days_since_epoch + 4) % 7 {
        0 => "Sun",
        1 => "Mon",
        2 => "Tue",
        3 => "Wed",
        4 => "Thu",
        5 => "Fri",
        6 => "Sat",
        _ => "Thu",
    };

    let mut days = days_since_epoch as i64;
    let mut year = 1970i64;
    loop {
        let leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
        let days_in_year = if leap { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
    let days_in_months = [
        ("Jan", 31),
        ("Feb", if leap { 29 } else { 28 }),
        ("Mar", 31),
        ("Apr", 30),
        ("May", 31),
        ("Jun", 30),
        ("Jul", 31),
        ("Aug", 31),
        ("Sep", 30),
        ("Oct", 31),
        ("Nov", 30),
        ("Dec", 31),
    ];

    let mut month_str = "Jan";
    let mut day_of_month = days + 1;
    for &(m, m_days) in &days_in_months {
        if day_of_month <= m_days {
            month_str = m;
            break;
        }
        day_of_month -= m_days;
    }

    format!(
        "{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
        day_of_week, day_of_month, month_str, year, hour, min, sec
    )
}

#[derive(Debug, PartialEq, Eq)]
pub enum RangeHeader {
    Satisfiable(u64, u64),
    Unsatisfiable,
    None,
}

pub fn parse_range_header(range_str: &str, file_len: u64) -> RangeHeader {
    let trimmed = range_str.trim();
    if !trimmed.starts_with("bytes=") {
        return RangeHeader::None;
    }
    let range_spec = &trimmed["bytes=".len()..];
    let first_range = range_spec.split(',').next().unwrap_or("").trim();
    if let Some(dash_pos) = first_range.find('-') {
        let start_str = first_range[..dash_pos].trim();
        let end_str = first_range[dash_pos + 1..].trim();

        if start_str.is_empty() {
            if let Ok(suffix_len) = end_str.parse::<u64>() {
                if suffix_len == 0 {
                    return RangeHeader::Unsatisfiable;
                }
                let start = if suffix_len >= file_len { 0 } else { file_len - suffix_len };
                let end = if file_len > 0 { file_len - 1 } else { 0 };
                return RangeHeader::Satisfiable(start, end);
            }
        } else if end_str.is_empty() {
            if let Ok(start) = start_str.parse::<u64>() {
                if start >= file_len {
                    return RangeHeader::Unsatisfiable;
                }
                let end = if file_len > 0 { file_len - 1 } else { 0 };
                return RangeHeader::Satisfiable(start, end);
            }
        } else if let (Ok(start), Ok(end)) = (start_str.parse::<u64>(), end_str.parse::<u64>()) {
            if start > end || start >= file_len {
                return RangeHeader::Unsatisfiable;
            }
            let end = end.min(if file_len > 0 { file_len - 1 } else { 0 });
            return RangeHeader::Satisfiable(start, end);
        }
    }
    RangeHeader::Unsatisfiable
}

pub fn serve_static_file(
    res_ptr: *mut c_void,
    file_path: &Path,
    req_headers: &HashMap<String, String>,
) -> bool {
    if !file_path.exists() || !file_path.is_file() {
        return false;
    }

    let metadata = match fs::metadata(file_path) {
        Ok(m) => m,
        Err(_) => return false,
    };

    let file_len = metadata.len();
    let mtime = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let etag = calculate_etag(file_len, mtime);
    let last_modified = format_http_date(mtime);

    let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let mime = get_mime_type_for_extension(ext);

    // 1. Conditional GET: If-None-Match
    if let Some(if_none_match) = req_headers.get("if-none-match") {
        if if_none_match.contains(&etag) || if_none_match.trim() == "*" {
            unsafe {
                let status = CString::new("304 Not Modified").unwrap();
                er_http_response_write_status(res_ptr, status.as_ptr(), status.as_bytes().len());
                let etag_key = CString::new("ETag").unwrap();
                let etag_val = CString::new(etag).unwrap();
                er_http_response_write_header(res_ptr, etag_key.as_ptr(), etag_key.as_bytes().len(), etag_val.as_ptr(), etag_val.as_bytes().len());
                er_http_response_end(res_ptr, b"".as_ptr() as *const c_char, 0);
            }
            return true;
        }
    }

    // 2. Conditional GET: If-Modified-Since
    if let Some(if_mod_since) = req_headers.get("if-modified-since") {
        if if_mod_since.trim() == last_modified.as_str() {
            unsafe {
                let status = CString::new("304 Not Modified").unwrap();
                er_http_response_write_status(res_ptr, status.as_ptr(), status.as_bytes().len());
                let etag_key = CString::new("ETag").unwrap();
                let etag_val = CString::new(etag).unwrap();
                er_http_response_write_header(res_ptr, etag_key.as_ptr(), etag_key.as_bytes().len(), etag_val.as_ptr(), etag_val.as_bytes().len());
                er_http_response_end(res_ptr, b"".as_ptr() as *const c_char, 0);
            }
            return true;
        }
    }

    // 3. Range Requests
    if let Some(range_header) = req_headers.get("range") {
        match parse_range_header(range_header, file_len) {
            RangeHeader::Satisfiable(start, end) => {
                let chunk_len = (end - start + 1) as usize;
                let mut buffer = vec![0u8; chunk_len];
                use std::io::{Read, Seek, SeekFrom};
                if let Ok(mut f) = fs::File::open(file_path) {
                    if f.seek(SeekFrom::Start(start)).is_ok() && f.read_exact(&mut buffer).is_ok() {
                        unsafe {
                            let status = CString::new("206 Partial Content").unwrap();
                            er_http_response_write_status(res_ptr, status.as_ptr(), status.as_bytes().len());
                            
                            let ct_k = CString::new("Content-Type").unwrap();
                            let ct_v = CString::new(mime).unwrap();
                            er_http_response_write_header(res_ptr, ct_k.as_ptr(), ct_k.as_bytes().len(), ct_v.as_ptr(), ct_v.as_bytes().len());
                            
                            let cr_k = CString::new("Content-Range").unwrap();
                            let cr_v = CString::new(format!("bytes {}-{}/{}", start, end, file_len)).unwrap();
                            er_http_response_write_header(res_ptr, cr_k.as_ptr(), cr_k.as_bytes().len(), cr_v.as_ptr(), cr_v.as_bytes().len());
                            
                            let cl_k = CString::new("Content-Length").unwrap();
                            let cl_v = CString::new(format!("{}", chunk_len)).unwrap();
                            er_http_response_write_header(res_ptr, cl_k.as_ptr(), cl_k.as_bytes().len(), cl_v.as_ptr(), cl_v.as_bytes().len());

                            let ar_k = CString::new("Accept-Ranges").unwrap();
                            let ar_v = CString::new("bytes").unwrap();
                            er_http_response_write_header(res_ptr, ar_k.as_ptr(), ar_k.as_bytes().len(), ar_v.as_ptr(), ar_v.as_bytes().len());

                            let etag_k = CString::new("ETag").unwrap();
                            let etag_v = CString::new(etag).unwrap();
                            er_http_response_write_header(res_ptr, etag_k.as_ptr(), etag_k.as_bytes().len(), etag_v.as_ptr(), etag_v.as_bytes().len());

                            er_http_response_end(res_ptr, buffer.as_ptr() as *const c_char, buffer.len());
                        }
                        return true;
                    }
                }
            }
            RangeHeader::Unsatisfiable => {
                unsafe {
                    let status = CString::new("416 Range Not Satisfiable").unwrap();
                    er_http_response_write_status(res_ptr, status.as_ptr(), status.as_bytes().len());
                    let cr_k = CString::new("Content-Range").unwrap();
                    let cr_v = CString::new(format!("bytes */{}", file_len)).unwrap();
                    er_http_response_write_header(res_ptr, cr_k.as_ptr(), cr_k.as_bytes().len(), cr_v.as_ptr(), cr_v.as_bytes().len());
                    er_http_response_end(res_ptr, b"".as_ptr() as *const c_char, 0);
                }
                return true;
            }
            RangeHeader::None => {}
        }
    }

    // 4. Full file response
    if let Ok(content) = fs::read(file_path) {
        unsafe {
            let status = CString::new("200 OK").unwrap();
            er_http_response_write_status(res_ptr, status.as_ptr(), status.as_bytes().len());

            let ct_k = CString::new("Content-Type").unwrap();
            let ct_v = CString::new(mime).unwrap();
            er_http_response_write_header(res_ptr, ct_k.as_ptr(), ct_k.as_bytes().len(), ct_v.as_ptr(), ct_v.as_bytes().len());

            let ar_k = CString::new("Accept-Ranges").unwrap();
            let ar_v = CString::new("bytes").unwrap();
            er_http_response_write_header(res_ptr, ar_k.as_ptr(), ar_k.as_bytes().len(), ar_v.as_ptr(), ar_v.as_bytes().len());

            let etag_k = CString::new("ETag").unwrap();
            let etag_v = CString::new(etag).unwrap();
            er_http_response_write_header(res_ptr, etag_k.as_ptr(), etag_k.as_bytes().len(), etag_v.as_ptr(), etag_v.as_bytes().len());

            let lm_k = CString::new("Last-Modified").unwrap();
            let lm_v = CString::new(last_modified).unwrap();
            er_http_response_write_header(res_ptr, lm_k.as_ptr(), lm_k.as_bytes().len(), lm_v.as_ptr(), lm_v.as_bytes().len());

            let cl_k = CString::new("Content-Length").unwrap();
            let cl_v = CString::new(format!("{}", content.len())).unwrap();
            er_http_response_write_header(res_ptr, cl_k.as_ptr(), cl_k.as_bytes().len(), cl_v.as_ptr(), cl_v.as_bytes().len());

            er_http_response_end(res_ptr, content.as_ptr() as *const c_char, content.len());
        }
        return true;
    }

    false
}

pub fn native_static_route_handler(_args: Vec<Value>) -> Value {
    let res_ptr = ACTIVE_HTTP_RESPONSE.with(|resp| resp.get());
    if res_ptr.is_null() {
        return Value::null();
    }
    let active_headers = ACTIVE_REQUEST_HEADERS.with(|h| h.borrow().clone());
    let req_path = ACTIVE_REQUEST_PATH.with(|p| p.borrow().clone());

    let mut served = false;
    STATIC_MOUNTS.with(|mounts| {
        for (prefix, root) in mounts.borrow().iter() {
            if req_path == *prefix || req_path.starts_with(&format!("{}/", prefix)) {
                let rel = if req_path == *prefix {
                    ""
                } else {
                    &req_path[prefix.len() + 1..]
                };
                
                let decoded_rel = percent_decode(rel);
                if decoded_rel.contains("..") {
                    continue;
                }
                
                let base = Path::new(root);
                let mut target_path = if decoded_rel.is_empty() {
                    base.join("index.html")
                } else {
                    base.join(&decoded_rel)
                };

                if target_path.is_dir() {
                    target_path = target_path.join("index.html");
                }

                if target_path.exists() && target_path.is_file() {
                    if serve_static_file(res_ptr, &target_path, &active_headers) {
                        served = true;
                        break;
                    }
                }
            }
        }
    });

    if !served {
        unsafe {
            let status = CString::new("404 Not Found").unwrap();
            er_http_response_write_status(res_ptr, status.as_ptr(), status.as_bytes().len());
            let not_found = "404 Not Found";
            er_http_response_end(res_ptr, not_found.as_ptr() as *const c_char, not_found.len());
        }
    }

    Value::null()
}

pub fn status_code_to_status_line(code: u16) -> &'static str {
    match code {
        200 => "200 OK",
        201 => "201 Created",
        202 => "202 Accepted",
        204 => "204 No Content",
        206 => "206 Partial Content",
        301 => "301 Moved Permanently",
        302 => "302 Found",
        303 => "303 See Other",
        304 => "304 Not Modified",
        307 => "307 Temporary Redirect",
        308 => "308 Permanent Redirect",
        400 => "400 Bad Request",
        401 => "401 Unauthorized",
        403 => "403 Forbidden",
        404 => "404 Not Found",
        405 => "405 Method Not Allowed",
        409 => "409 Conflict",
        416 => "416 Range Not Satisfiable",
        429 => "429 Too Many Requests",
        500 => "500 Internal Server Error",
        502 => "502 Bad Gateway",
        503 => "503 Service Unavailable",
        _ => "200 OK",
    }
}

pub fn flush_response(
    res_ptr: *mut c_void,
    body: Option<&[u8]>,
    default_content_type: Option<&str>,
    default_status: u16,
) -> bool {
    if res_ptr.is_null() {
        return false;
    }

    let (status_code, headers, cookies, already_finished) = ACTIVE_RESPONSE_STATE.with(|state| {
        let mut s = state.borrow_mut();
        if s.finished {
            return (0, Vec::new(), Vec::new(), true);
        }
        s.finished = true;
        (
            s.status.unwrap_or(default_status),
            s.headers.clone(),
            s.cookies.clone(),
            false,
        )
    });

    if already_finished {
        return false;
    }

    unsafe {
        // 1. Status
        let status_line = status_code_to_status_line(status_code);
        let s_c = CString::new(status_line).unwrap();
        er_http_response_write_status(res_ptr, s_c.as_ptr(), s_c.as_bytes().len());

        // 2. Content-Type
        let mut has_content_type = false;
        for (k, _) in &headers {
            if k.eq_ignore_ascii_case("content-type") {
                has_content_type = true;
                break;
            }
        }
        if !has_content_type {
            if let Some(ct) = default_content_type {
                let k_c = CString::new("Content-Type").unwrap();
                let v_c = CString::new(ct).unwrap();
                er_http_response_write_header(res_ptr, k_c.as_ptr(), k_c.as_bytes().len(), v_c.as_ptr(), v_c.as_bytes().len());
            }
        }

        // 3. Headers
        for (k, v) in headers {
            let k_c = CString::new(k).unwrap();
            let v_c = CString::new(v).unwrap();
            er_http_response_write_header(res_ptr, k_c.as_ptr(), k_c.as_bytes().len(), v_c.as_ptr(), v_c.as_bytes().len());
        }

        // 4. Cookies
        for cookie_str in cookies {
            let k_c = CString::new("Set-Cookie").unwrap();
            let v_c = CString::new(cookie_str).unwrap();
            er_http_response_write_header(res_ptr, k_c.as_ptr(), k_c.as_bytes().len(), v_c.as_ptr(), v_c.as_bytes().len());
        }

        // 5. Body
        let body_bytes = body.unwrap_or(b"");
        er_http_response_end(res_ptr, body_bytes.as_ptr() as *const c_char, body_bytes.len());
    }

    true
}

pub fn native_context_json(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let res_ptr = ACTIVE_HTTP_RESPONSE.with(|resp| resp.get());
    if res_ptr.is_null() {
        return Value::null();
    }
    let data = args[0];
    if args.len() >= 2 && args[1].is_number() {
        ACTIVE_RESPONSE_STATE.with(|s| s.borrow_mut().status = Some(args[1].as_number() as u16));
    }
    let json_val = value_to_json(data);
    let json_str = serde_json::to_string(&json_val).unwrap_or_else(|_| "null".to_string());
    flush_response(res_ptr, Some(json_str.as_bytes()), Some("application/json"), 200);
    Value::null()
}

pub fn native_context_html(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let res_ptr = ACTIVE_HTTP_RESPONSE.with(|resp| resp.get());
    if res_ptr.is_null() {
        return Value::null();
    }
    let html_val = args[0];
    if args.len() >= 2 && args[1].is_number() {
        ACTIVE_RESPONSE_STATE.with(|s| s.borrow_mut().status = Some(args[1].as_number() as u16));
    }
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
    flush_response(res_ptr, Some(html_str.as_bytes()), Some("text/html; charset=utf-8"), 200);
    Value::null()
}

pub fn native_context_text(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let res_ptr = ACTIVE_HTTP_RESPONSE.with(|resp| resp.get());
    if res_ptr.is_null() {
        return Value::null();
    }
    let text_val = args[0];
    if args.len() >= 2 && args[1].is_number() {
        ACTIVE_RESPONSE_STATE.with(|s| s.borrow_mut().status = Some(args[1].as_number() as u16));
    }
    let text_str = if text_val.is_string() {
        unsafe {
            match &(*text_val.as_gc_ptr()).data {
                GcData::String(s) => s.as_ref().to_string(),
                _ => return Value::null(),
            }
        }
    } else {
        text_val.to_string()
    };
    flush_response(res_ptr, Some(text_str.as_bytes()), Some("text/plain; charset=utf-8"), 200);
    Value::null()
}

pub fn native_context_status(args: Vec<Value>) -> Value {
    if !args.is_empty() && args[0].is_number() {
        let code = args[0].as_number() as u16;
        ACTIVE_RESPONSE_STATE.with(|s| s.borrow_mut().status = Some(code));
    }
    Value::null()
}

pub fn native_context_header(args: Vec<Value>) -> Value {
    if args.len() == 1 {
        let name = match args[0].as_str() {
            Some(s) => s.to_ascii_lowercase(),
            None => return Value::null(),
        };
        let val_opt = ACTIVE_REQUEST_HEADERS.with(|h| h.borrow().get(&name).cloned());
        if let Some(val) = val_opt {
            let ptr = get_or_create_string(&val);
            Value::string(ptr)
        } else {
            Value::null()
        }
    } else if args.len() >= 2 {
        let name = match args[0].as_str() {
            Some(s) => s.to_string(),
            None => return Value::null(),
        };
        let val = match args[1].as_str() {
            Some(s) => s.to_string(),
            None => args[1].to_string(),
        };
        ACTIVE_RESPONSE_STATE.with(|s| {
            s.borrow_mut().headers.push((name, val));
        });
        Value::null()
    } else {
        Value::null()
    }
}

pub fn native_req_header(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let name = match args[0].as_str() {
        Some(s) => s.to_ascii_lowercase(),
        None => return Value::null(),
    };
    let val_opt = ACTIVE_REQUEST_HEADERS.with(|h| h.borrow().get(&name).cloned());
    if let Some(val) = val_opt {
        let ptr = get_or_create_string(&val);
        Value::string(ptr)
    } else {
        Value::null()
    }
}

pub fn native_req_cookie(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let name = match args[0].as_str() {
        Some(s) => s,
        None => return Value::null(),
    };
    let val_opt = ACTIVE_REQUEST_COOKIES.with(|c| c.borrow().get(name).cloned());
    if let Some(val) = val_opt {
        let ptr = get_or_create_string(&val);
        Value::string(ptr)
    } else {
        Value::null()
    }
}

pub fn native_req_query(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let name = match args[0].as_str() {
        Some(s) => s,
        None => return Value::null(),
    };
    let val_opt = ACTIVE_REQUEST_QUERY.with(|q| q.borrow().get(name).cloned());
    if let Some(val) = val_opt {
        let ptr = get_or_create_string(&val);
        Value::string(ptr)
    } else {
        Value::null()
    }
}

pub fn native_res_get_header(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let name = match args[0].as_str() {
        Some(s) => s.to_ascii_lowercase(),
        None => return Value::null(),
    };
    let val_opt = ACTIVE_RESPONSE_STATE.with(|s| {
        for (k, v) in &s.borrow().headers {
            if k.eq_ignore_ascii_case(&name) {
                return Some(v.clone());
            }
        }
        None
    });
    if let Some(val) = val_opt {
        let ptr = get_or_create_string(&val);
        Value::string(ptr)
    } else {
        Value::null()
    }
}

pub fn native_context_set_cookie(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::null();
    }
    let name = match args[0].as_str() {
        Some(s) => s,
        None => return Value::null(),
    };
    let val = match args[1].as_str() {
        Some(s) => s.to_string(),
        None => args[1].to_string(),
    };
    let options = if args.len() >= 3 { Some(args[2]) } else { None };
    let cookie_str = format_cookie(name, &val, options);
    ACTIVE_RESPONSE_STATE.with(|s| {
        s.borrow_mut().cookies.push(cookie_str);
    });
    Value::null()
}

pub fn native_context_clear_cookie(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let name = match args[0].as_str() {
        Some(s) => s,
        None => return Value::null(),
    };
    let path = if args.len() >= 2 && args[1].is_object() {
        let ptr = args[1].as_gc_ptr();
        unsafe {
            if let GcData::Object(map) = &(*ptr).data {
                map.get(&crate::vm::value::MapKey(Value::string(get_or_create_string("path"))))
                    .and_then(|v| v.as_str())
                    .unwrap_or("/")
            } else {
                "/"
            }
        }
    } else {
        "/"
    };
    let cookie_str = format!("{}=; Path={}; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:00 GMT", name, path);
    ACTIVE_RESPONSE_STATE.with(|s| {
        s.borrow_mut().cookies.push(cookie_str);
    });
    Value::null()
}

pub fn native_context_redirect(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let res_ptr = ACTIVE_HTTP_RESPONSE.with(|resp| resp.get());
    if res_ptr.is_null() {
        return Value::null();
    }
    let url_str = match args[0].as_str() {
        Some(s) => s.to_string(),
        None => return Value::null(),
    };
    let status_code = if args.len() >= 2 && args[1].is_number() {
        args[1].as_number() as u16
    } else {
        302
    };
    ACTIVE_RESPONSE_STATE.with(|s| {
        s.borrow_mut().headers.push(("Location".to_string(), url_str));
    });
    flush_response(res_ptr, Some(b""), None, status_code);
    Value::null()
}

pub fn native_context_serve_static(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let res_ptr = ACTIVE_HTTP_RESPONSE.with(|resp| resp.get());
    if res_ptr.is_null() {
        return Value::null();
    }
    let path_str = match args[0].as_str() {
        Some(s) => s.to_string(),
        None => return Value::null(),
    };
    let headers = ACTIVE_REQUEST_HEADERS.with(|h| h.borrow().clone());
    let file_path = Path::new(&path_str);
    if !serve_static_file(res_ptr, file_path, &headers) {
        unsafe {
            let status = CString::new("404 Not Found").unwrap();
            er_http_response_write_status(res_ptr, status.as_ptr(), status.as_bytes().len());
            let not_found = "404 Not Found";
            er_http_response_end(res_ptr, not_found.as_ptr() as *const c_char, not_found.len());
        }
    }
    Value::null()
}

pub fn native_res_end(args: Vec<Value>) -> Value {
    let res_ptr = ACTIVE_HTTP_RESPONSE.with(|resp| resp.get());
    if res_ptr.is_null() {
        return Value::null();
    }
    let body_bytes = if !args.is_empty() {
        if let Some(s) = args[0].as_str() {
            s.as_bytes().to_vec()
        } else {
            args[0].to_string().into_bytes()
        }
    } else {
        Vec::new()
    };
    flush_response(res_ptr, Some(&body_bytes), None, 200);
    Value::null()
}

pub fn get_target_script_path() -> Option<String> {
    TARGET_SCRIPT_PATH.with(|p| p.borrow().clone())
}


pub fn end_http_response_json(res: *mut std::ffi::c_void, json: &str) {
    if res.is_null() {
        return;
    }
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
        let toml_path = parent.join("eronom.toml");
        if toml_path.exists() {
            if let Ok(meta) = fs::metadata(&toml_path) {
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
    
    // Safely free old MIR JIT buffers and clear code caches on reload
    crate::jit::reset_jit_state();

    let old_routes = ROUTES.with(|r| std::mem::take(&mut *r.borrow_mut()));
    let old_ws_routes = WS_ROUTES.with(|r| std::mem::take(&mut *r.borrow_mut()));
    let old_mws = MIDDLEWARES.with(|r| std::mem::take(&mut *r.borrow_mut()));
    let old_mounts = STATIC_MOUNTS.with(|r| std::mem::take(&mut *r.borrow_mut()));
    let old_listen_port = LISTEN_PORT.with(|p| p.replace(None));
    let old_listen_callback = LISTEN_CALLBACK.with(|cb| cb.replace(None));
    ROUTER.with(|r| r.borrow_mut().clear());
    
    let path_buf = Path::new(&path);
    let stmts = match crate::frontend::parse_and_resolve_imports(path_buf) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[HTTP] Reload error: Parsing/Import resolution failed: {}", e);
            ROUTES.with(|r| *r.borrow_mut() = old_routes.clone());
            WS_ROUTES.with(|r| *r.borrow_mut() = old_ws_routes);
            MIDDLEWARES.with(|r| *r.borrow_mut() = old_mws);
            STATIC_MOUNTS.with(|r| *r.borrow_mut() = old_mounts);
            LISTEN_PORT.with(|p| p.set(old_listen_port));
            LISTEN_CALLBACK.with(|cb| *cb.borrow_mut() = old_listen_callback);
            ROUTER.with(|r| {
                let mut router = r.borrow_mut();
                router.clear();
                for route in &old_routes {
                    router.insert(&route.method, &route.path, route.callback);
                }
            });
            return;
        }
    };
    
    let compiler = crate::vm::compiler::Compiler::new();
    let function = match compiler.compile(&stmts) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[HTTP] Reload error: Compilation failed: {}", e);
            ROUTES.with(|r| *r.borrow_mut() = old_routes.clone());
            WS_ROUTES.with(|r| *r.borrow_mut() = old_ws_routes);
            MIDDLEWARES.with(|r| *r.borrow_mut() = old_mws);
            STATIC_MOUNTS.with(|r| *r.borrow_mut() = old_mounts);
            LISTEN_PORT.with(|p| p.set(old_listen_port));
            LISTEN_CALLBACK.with(|cb| *cb.borrow_mut() = old_listen_callback);
            ROUTER.with(|r| {
                let mut router = r.borrow_mut();
                router.clear();
                for route in &old_routes {
                    router.insert(&route.method, &route.path, route.callback);
                }
            });
            return;
        }
    };
    
    // Reload eronom.toml if it exists
    let parent_dir = Path::new(&path).parent();
    if let Some(parent) = parent_dir {
        let toml_path = parent.join("eronom.toml");
        if toml_path.exists() {
            if let Ok(toml_content) = fs::read_to_string(&toml_path) {
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
        eprintln!("[HTTP] Reload error: Execution failed: {}", e);
        ROUTES.with(|r| *r.borrow_mut() = old_routes.clone());
        WS_ROUTES.with(|r| *r.borrow_mut() = old_ws_routes);
        MIDDLEWARES.with(|r| *r.borrow_mut() = old_mws);
        STATIC_MOUNTS.with(|r| *r.borrow_mut() = old_mounts);
        LISTEN_PORT.with(|p| p.set(old_listen_port));
        LISTEN_CALLBACK.with(|cb| *cb.borrow_mut() = old_listen_callback);
        ROUTER.with(|r| {
            let mut router = r.borrow_mut();
            router.clear();
            for route in &old_routes {
                router.insert(&route.method, &route.path, route.callback);
            }
        });
        return;
    }
    
    LAST_MTIME.with(|m| m.set(Some(current_mtime)));
    println!("[HTTP] Reload successful. VM state and routes updated.");
}

fn match_route_path(pattern: &str, path: &str) -> Option<HashMap<String, String>> {
    if pattern == "*" || pattern == "/*" {
        let mut params = HashMap::new();
        let tail = if path.starts_with('/') { &path[1..] } else { path };
        params.insert("*".to_string(), tail.to_string());
        return Some(params);
    }
    
    let pattern_parts: Vec<&str> = pattern.split('/').collect();
    let path_parts: Vec<&str> = path.split('/').collect();
    
    let mut params = HashMap::new();
    for (i, pat_part) in pattern_parts.iter().enumerate() {
        if *pat_part == "*" || pat_part.starts_with('*') {
            if i == pattern_parts.len() - 1 {
                let tail = if i < path_parts.len() {
                    path_parts[i..].join("/")
                } else {
                    String::new()
                };
                let key = if *pat_part == "*" { "*" } else { &pat_part[1..] };
                params.insert(key.to_string(), tail);
                return Some(params);
            }
        }
        
        if i >= path_parts.len() {
            return None;
        }
        
        let path_part = path_parts[i];
        if pat_part.starts_with(':') {
            let param_name = &pat_part[1..];
            params.insert(param_name.to_string(), path_part.to_string());
        } else if pat_part != &path_part {
            return None;
        }
    }
    
    if pattern_parts.len() == path_parts.len() {
        Some(params)
    } else {
        None
    }
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
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

struct MultipartPart {
    headers: HashMap<String, String>,
    data: Vec<u8>,
}

fn parse_header_params(header_val: &str) -> HashMap<String, String> {
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

fn parse_part_bytes(part_bytes: &[u8]) -> Option<MultipartPart> {
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

fn parse_multipart(body: &[u8], boundary: &str) -> Vec<MultipartPart> {
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

fn construct_file_object(vm: &mut VM, name: &str, type_str: &str, size: usize) -> Value {
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

#[unsafe(no_mangle)]
pub extern "C" fn er_http_on_request(
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
    let headers_str = unsafe {
        if headers_ptr.is_null() {
            ""
        } else {
            let slice = std::slice::from_raw_parts(headers_ptr as *const u8, headers_len);
            std::str::from_utf8(slice).unwrap_or("")
        }
    };
    let body_bytes = unsafe {
        if body_ptr.is_null() {
            &[]
        } else {
            std::slice::from_raw_parts(body_ptr as *const u8, body_len)
        }
    };

    let (clean_path, raw_query) = match path.find('?') {
        Some(idx) => (&path[..idx], &path[idx + 1..]),
        None => (path, ""),
    };

    ACTIVE_REQUEST_PATH.with(|p| {
        *p.borrow_mut() = clean_path.to_string();
    });

    let mut extracted_params = HashMap::new();
    let mut callback_opt = ROUTER.with(|router| {
        if let Some((handler, params)) = router.borrow().find(method, clean_path) {
            extracted_params = params;
            Some(handler)
        } else {
            None
        }
    });

    if callback_opt.is_none() {
        callback_opt = ROUTES.with(|routes| {
            for route in routes.borrow().iter() {
                if route.method == "ALL" || route.method == "*" || route.method.eq_ignore_ascii_case(method) {
                    if let Some(params) = match_route_path(&route.path, clean_path) {
                        extracted_params = params;
                        return Some(route.callback);
                    }
                }
            }
            None
        });
    }

    let parsed_query = parse_query_string(raw_query);
    ACTIVE_REQUEST_QUERY.with(|q| {
        *q.borrow_mut() = parsed_query.clone();
    });

    let mut headers_map = HashMap::new();
    for line in headers_str.lines() {
        if let Some(pos) = line.find(": ") {
            let key = line[..pos].to_lowercase();
            let val = line[pos + 2..].to_string();
            headers_map.insert(key, val);
        }
    }
    ACTIVE_REQUEST_HEADERS.with(|h| {
        *h.borrow_mut() = headers_map.clone();
    });

    let cookie_header = headers_map.get("cookie").map(|s| s.as_str()).unwrap_or("");
    let parsed_cookies = parse_cookies(cookie_header);
    ACTIVE_REQUEST_COOKIES.with(|c| {
        *c.borrow_mut() = parsed_cookies.clone();
    });

    ACTIVE_RESPONSE_STATE.with(|s| {
        s.borrow_mut().reset();
    });

    if let Some(callback) = callback_opt {
        ACTIVE_HTTP_RESPONSE.with(|resp| {
            resp.set(res);
        });

        ACTIVE_VM.with(|active| {
            let vm_ptr = active.get();
            if !vm_ptr.is_null() {
                let vm = unsafe { &mut *vm_ptr };

                let mut req_map = crate::vm::gc::get_pooled_map(14);
                let url_name = get_or_create_string("url");
                let path_name = get_or_create_string("path");
                let method_name = get_or_create_string("method");
                let params_name = get_or_create_string("params");
                let query_name = get_or_create_string("query");
                let raw_query_name = get_or_create_string("rawQuery");
                let headers_name = get_or_create_string("headers");
                let header_name = get_or_create_string("header");
                let cookies_name = get_or_create_string("cookies");
                let cookie_name = get_or_create_string("cookie");
                let files_name = get_or_create_string("files");
                let body_name = get_or_create_string("_body");
                let query_fn_name = get_or_create_string("queryParam");

                let full_url_str = get_or_create_string(path);
                let clean_path_str = get_or_create_string(clean_path);
                let raw_query_str = get_or_create_string(raw_query);
                let method_str = get_or_create_string(method);

                // Parameters map
                let mut params_obj_map = crate::vm::gc::get_pooled_map(extracted_params.len());
                for (k, v) in extracted_params {
                    let k_str = get_or_create_string(&k);
                    let v_str = get_or_create_string(&v);
                    params_obj_map.insert(crate::vm::value::MapKey(Value::string(k_str)), Value::string(v_str));
                }
                let params_obj = Value::object(crate::vm::gc::gc_allocate(GcData::Object(params_obj_map)));

                // Query map
                let mut query_obj_map = crate::vm::gc::get_pooled_map(parsed_query.len());
                for (k, v) in &parsed_query {
                    let k_str = get_or_create_string(k);
                    let v_str = get_or_create_string(v);
                    query_obj_map.insert(crate::vm::value::MapKey(Value::string(k_str)), Value::string(v_str));
                }
                let query_obj = Value::object(crate::vm::gc::gc_allocate(GcData::Object(query_obj_map)));

                // Headers map
                let mut headers_obj_map = crate::vm::gc::get_pooled_map(headers_map.len());
                for (k, v) in &headers_map {
                    let k_str = get_or_create_string(k);
                    let v_str = get_or_create_string(v);
                    headers_obj_map.insert(crate::vm::value::MapKey(Value::string(k_str)), Value::string(v_str));
                }
                let headers_obj = Value::object(crate::vm::gc::gc_allocate(GcData::Object(headers_obj_map)));

                // Cookies map
                let mut cookies_obj_map = crate::vm::gc::get_pooled_map(parsed_cookies.len());
                for (k, v) in &parsed_cookies {
                    let k_str = get_or_create_string(k);
                    let v_str = get_or_create_string(v);
                    cookies_obj_map.insert(crate::vm::value::MapKey(Value::string(k_str)), Value::string(v_str));
                }
                let cookies_obj = Value::object(crate::vm::gc::gc_allocate(GcData::Object(cookies_obj_map)));

                // Multipart files parsing
                let mut content_type = String::new();
                for (k, v) in &headers_map {
                    if k == "content-type" {
                        content_type = v.clone();
                        break;
                    }
                }

                let mut files_obj_map = crate::vm::gc::get_pooled_map(5);
                if content_type.contains("multipart/form-data") {
                    if let Some(b_pos) = content_type.find("boundary=") {
                        let boundary = content_type[b_pos + 9..].trim().to_string();
                        let parts = parse_multipart(body_bytes, &boundary);

                        let temp_dir = std::path::Path::new("temp_uploads");
                        if !temp_dir.exists() {
                            let _ = std::fs::create_dir_all(temp_dir);
                        }

                        static UPLOAD_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

                        for part in parts {
                            if let Some(cd) = part.headers.get("content-disposition") {
                                let params = parse_header_params(cd);
                                if let Some(name) = params.get("name") {
                                    if let Some(_filename) = params.get("filename") {
                                        let count = UPLOAD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                                        let temp_filename = format!("temp_uploads/upload_{}_{}.tmp", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis(), count);

                                        if std::fs::write(&temp_filename, &part.data).is_ok() {
                                            let content_type_str = part.headers.get("content-type").map(|s| s.as_str()).unwrap_or("application/octet-stream");
                                            let file_val = construct_file_object(vm, &temp_filename, content_type_str, part.data.len());
                                            files_obj_map.insert(
                                                crate::vm::value::MapKey(Value::string(get_or_create_string(name))),
                                                file_val
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                let files_obj = Value::object(crate::vm::gc::gc_allocate(GcData::Object(files_obj_map)));

                let body_str = std::str::from_utf8(body_bytes).unwrap_or("");
                let body_val = Value::string(get_or_create_string(body_str));

                req_map.insert(crate::vm::value::MapKey(Value::string(url_name)), Value::string(full_url_str));
                req_map.insert(crate::vm::value::MapKey(Value::string(path_name)), Value::string(clean_path_str));
                req_map.insert(crate::vm::value::MapKey(Value::string(method_name)), Value::string(method_str));
                req_map.insert(crate::vm::value::MapKey(Value::string(params_name)), params_obj);
                req_map.insert(crate::vm::value::MapKey(Value::string(query_name)), query_obj);
                req_map.insert(crate::vm::value::MapKey(Value::string(query_fn_name)), Value::native_function(native_req_query));
                req_map.insert(crate::vm::value::MapKey(Value::string(raw_query_name)), Value::string(raw_query_str));
                req_map.insert(crate::vm::value::MapKey(Value::string(headers_name)), headers_obj);
                req_map.insert(crate::vm::value::MapKey(Value::string(header_name)), Value::native_function(native_req_header));
                req_map.insert(crate::vm::value::MapKey(Value::string(cookies_name)), cookies_obj);
                req_map.insert(crate::vm::value::MapKey(Value::string(cookie_name)), Value::native_function(native_req_cookie));
                req_map.insert(crate::vm::value::MapKey(Value::string(files_name)), files_obj);
                req_map.insert(crate::vm::value::MapKey(Value::string(body_name)), body_val);

                let req_obj = Value::object(crate::vm::gc::gc_allocate(GcData::Object(req_map)));

                // Response object (c.res)
                let mut res_map = crate::vm::gc::get_pooled_map(12);
                let status_key = get_or_create_string("status");
                let set_status_key = get_or_create_string("setStatus");
                let header_key = get_or_create_string("header");
                let set_header_key = get_or_create_string("setHeader");
                let get_header_key = get_or_create_string("getHeader");
                let set_cookie_key = get_or_create_string("setCookie");
                let clear_cookie_key = get_or_create_string("clearCookie");
                let send_key = get_or_create_string("send");
                let text_key = get_or_create_string("text");
                let json_key = get_or_create_string("json");
                let html_key = get_or_create_string("html");
                let end_key = get_or_create_string("end");
                let redirect_key = get_or_create_string("redirect");

                res_map.insert(crate::vm::value::MapKey(Value::string(status_key)), Value::native_function(native_context_status));
                res_map.insert(crate::vm::value::MapKey(Value::string(set_status_key)), Value::native_function(native_context_status));
                res_map.insert(crate::vm::value::MapKey(Value::string(header_key)), Value::native_function(native_context_header));
                res_map.insert(crate::vm::value::MapKey(Value::string(set_header_key)), Value::native_function(native_context_header));
                res_map.insert(crate::vm::value::MapKey(Value::string(get_header_key)), Value::native_function(native_res_get_header));
                res_map.insert(crate::vm::value::MapKey(Value::string(set_cookie_key)), Value::native_function(native_context_set_cookie));
                res_map.insert(crate::vm::value::MapKey(Value::string(clear_cookie_key)), Value::native_function(native_context_clear_cookie));
                res_map.insert(crate::vm::value::MapKey(Value::string(send_key)), Value::native_function(native_context_text));
                res_map.insert(crate::vm::value::MapKey(Value::string(text_key)), Value::native_function(native_context_text));
                res_map.insert(crate::vm::value::MapKey(Value::string(json_key)), Value::native_function(native_context_json));
                res_map.insert(crate::vm::value::MapKey(Value::string(html_key)), Value::native_function(native_context_html));
                res_map.insert(crate::vm::value::MapKey(Value::string(end_key)), Value::native_function(native_res_end));
                res_map.insert(crate::vm::value::MapKey(Value::string(redirect_key)), Value::native_function(native_context_redirect));

                let res_obj = Value::object(crate::vm::gc::gc_allocate(GcData::Object(res_map)));

                // Context object (c)
                let mut map = crate::vm::gc::get_pooled_map(16);
                let json_name = get_or_create_string("json");
                let html_name = get_or_create_string("html");
                let text_name = get_or_create_string("text");
                let send_name = get_or_create_string("send");
                let status_name = get_or_create_string("status");
                let header_name_c = get_or_create_string("header");
                let set_header_name_c = get_or_create_string("setHeader");
                let cookie_name_c = get_or_create_string("cookie");
                let set_cookie_name_c = get_or_create_string("setCookie");
                let clear_cookie_name_c = get_or_create_string("clearCookie");
                let redirect_name_c = get_or_create_string("redirect");
                let serve_static_name_c = get_or_create_string("serveStatic");
                let file_name_c = get_or_create_string("file");
                let req_key_name = get_or_create_string("req");
                let res_key_name = get_or_create_string("res");

                map.insert(crate::vm::value::MapKey(Value::string(json_name)), Value::native_function(native_context_json));
                map.insert(crate::vm::value::MapKey(Value::string(html_name)), Value::native_function(native_context_html));
                map.insert(crate::vm::value::MapKey(Value::string(text_name)), Value::native_function(native_context_text));
                map.insert(crate::vm::value::MapKey(Value::string(send_name)), Value::native_function(native_context_text));
                map.insert(crate::vm::value::MapKey(Value::string(status_name)), Value::native_function(native_context_status));
                map.insert(crate::vm::value::MapKey(Value::string(header_name_c)), Value::native_function(native_context_header));
                map.insert(crate::vm::value::MapKey(Value::string(set_header_name_c)), Value::native_function(native_context_header));
                map.insert(crate::vm::value::MapKey(Value::string(cookie_name_c)), Value::native_function(native_req_cookie));
                map.insert(crate::vm::value::MapKey(Value::string(set_cookie_name_c)), Value::native_function(native_context_set_cookie));
                map.insert(crate::vm::value::MapKey(Value::string(clear_cookie_name_c)), Value::native_function(native_context_clear_cookie));
                map.insert(crate::vm::value::MapKey(Value::string(redirect_name_c)), Value::native_function(native_context_redirect));
                map.insert(crate::vm::value::MapKey(Value::string(serve_static_name_c)), Value::native_function(native_context_serve_static));
                map.insert(crate::vm::value::MapKey(Value::string(file_name_c)), Value::native_function(native_context_serve_static));
                map.insert(crate::vm::value::MapKey(Value::string(req_key_name)), req_obj);
                map.insert(crate::vm::value::MapKey(Value::string(res_key_name)), res_obj);

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
            let status = CString::new("404 Not Found").unwrap();
            er_http_response_write_status(res, status.as_ptr(), status.as_bytes().len());
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
        if path_ptr.is_null() || path_len == 0 {
            ""
        } else {
            let slice = std::slice::from_raw_parts(path_ptr as *const u8, path_len);
            std::str::from_utf8(slice).unwrap_or("")
        }
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
    is_binary: i32,
) {
    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if !vm_ptr.is_null() {
        let vm = unsafe { &mut *vm_ptr };
        check_and_reload_script_if_needed(vm);
    }

    let path = unsafe {
        if path_ptr.is_null() || path_len == 0 {
            ""
        } else {
            let slice = std::slice::from_raw_parts(path_ptr as *const u8, path_len);
            std::str::from_utf8(slice).unwrap_or("")
        }
    };
    
    let msg_bytes = unsafe {
        if message_ptr.is_null() || message_len == 0 {
            &[]
        } else {
            std::slice::from_raw_parts(message_ptr as *const u8, message_len)
        }
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
                let is_bin_bool = is_binary != 0;
                let msg_val = if is_bin_bool {
                    let mut byte_vals = crate::vm::gc::get_pooled_vec(msg_bytes.len());
                    for &b in msg_bytes {
                        byte_vals.push(Value::number(b as f64));
                    }
                    let arr_ptr = crate::vm::gc::gc_allocate(GcData::Array(byte_vals));
                    Value::array(arr_ptr)
                } else {
                    let msg_str_slice = std::str::from_utf8(msg_bytes).unwrap_or("");
                    let msg_str = get_or_create_string(msg_str_slice);
                    Value::string(msg_str)
                };
                let is_bin_val = Value::boolean(is_bin_bool);
                
                if let Err(e) = vm.call_function_reentrant(callback, vec![ws_obj, msg_val, is_bin_val]) {
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
        if path_ptr.is_null() || path_len == 0 {
            ""
        } else {
            let slice = std::slice::from_raw_parts(path_ptr as *const u8, path_len);
            std::str::from_utf8(slice).unwrap_or("")
        }
    };
    
    let msg = unsafe {
        if message_ptr.is_null() || message_len == 0 {
            ""
        } else {
            let slice = std::slice::from_raw_parts(message_ptr as *const u8, message_len);
            std::str::from_utf8(slice).unwrap_or("")
        }
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

    let timer_id = vm.next_timer_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let due_time = std::time::Instant::now() + std::time::Duration::from_millis(delay_ms);

    vm.timers.lock().unwrap().push(crate::vm::execute::VmTimer {
        id: timer_id,
        due_time,
        action: crate::vm::execute::VmTimerAction::Callback {
            callback,
            args: cb_args,
        },
    });
    vm.active_async_tasks.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    vm.event_loop_condvar.notify_all();

    Value::number(timer_id as f64)
}

pub fn native_clear_timeout(args: Vec<Value>) -> Value {
    if args.is_empty() || !args[0].is_number() {
        return Value::null();
    }
    let timer_id = args[0].as_number() as u64;

    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if vm_ptr.is_null() {
        return Value::null();
    }
    let vm = unsafe { &mut *vm_ptr };

    let mut timers = vm.timers.lock().unwrap();
    let mut remaining: Vec<crate::vm::execute::VmTimer> = timers.drain().collect();
    if let Some(pos) = remaining.iter().position(|t| t.id == timer_id) {
        remaining.swap_remove(pos);
        vm.active_async_tasks.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
    for t in remaining {
        timers.push(t);
    }

    Value::null()
}

fn get_ureq_agent() -> &'static ureq::Agent {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| {
        let config = ureq::config::Config::builder()
            .timeout_global(Some(std::time::Duration::from_secs(10)))
            .user_agent("Eronom/0.9.2")
            .max_idle_connections(100)
            .max_idle_connections_per_host(100)
            .build();
        config.new_agent()
    })
}

fn perform_native_fetch(url: &str) -> Result<String, String> {
    let agent = get_ureq_agent();
    let mut resp = agent.get(url).call().map_err(|e| e.to_string())?;
    resp.body_mut().read_to_string().map_err(|e| e.to_string())
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

    match perform_native_fetch(&url_str) {
        Ok(body_str) => {
            let mut map = crate::vm::gc::get_pooled_map(2);
            let body_key = crate::vm::gc::get_or_create_string("_body");
            let body_val = crate::vm::gc::get_or_create_string(&body_str);
            map.insert(crate::vm::value::MapKey(Value::string(body_key)), Value::string(body_val));
            let ptr = crate::vm::gc::gc_allocate(crate::vm::gc::GcData::Object(map));
            Value::object(ptr)
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
    let condvar = vm.event_loop_condvar.clone();
    active_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    let promise_ptr_usize = promise_ptr as usize;

    // 4. Spawn background thread to fetch URL
    std::thread::spawn(move || {
        let promise_ptr = promise_ptr_usize as *mut crate::vm::gc::GcObject;
        let res = perform_native_fetch(&url_str);

        // Post ResolveFetchPromise back to event loop
        {
            let mut q = queue.lock().unwrap();
            q.push(crate::vm::execute::EventLoopTask {
                callback: Value::null(),
                args: Vec::new(),
                result: crate::vm::execute::AsyncResult::ResolveFetchPromise(promise_ptr, res),
            });
            condvar.notify_one();
        }

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

pub fn native_array_push(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::null();
    }
    let arr_val = args[0];
    let elem = args[1];
    if !arr_val.is_array() {
        return Value::null();
    }
    let arr_ptr = arr_val.as_gc_ptr();
    unsafe {
        match &mut (*arr_ptr).data {
            GcData::Array(arr) => {
                arr.push(elem);
            }
            _ => {}
        }
    }
    Value::null()
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

    let timer_id = vm.next_timer_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let due_time = std::time::Instant::now() + std::time::Duration::from_millis(delay_ms);

    vm.timers.lock().unwrap().push(crate::vm::execute::VmTimer {
        id: timer_id,
        due_time,
        action: crate::vm::execute::VmTimerAction::ResolvePromise {
            promise_ptr,
            value: Value::null(),
        },
    });
    vm.active_async_tasks.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    vm.event_loop_condvar.notify_all();

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
                crate::vm::gc::GcData::Object(map) => {
                    let is_file_key = crate::vm::gc::get_or_create_string("_isFile");
                    if let Some(is_file_val) = map.get(&crate::vm::value::MapKey(Value::string(is_file_key))) {
                        return Value::boolean(is_file_val.as_boolean());
                    }
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

pub fn native_file_global(args: Vec<Value>) -> Value {
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
        return Value::null();
    }
    let vm = unsafe { &mut *vm_ptr };

    let ext = std::path::Path::new(&path_str)
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

    let size = std::fs::metadata(&path_str)
        .map(|m| m.len())
        .unwrap_or(0);

    construct_file_object(vm, &path_str, mime, size as usize)
}

pub fn native_write_global(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::number(0.0);
    }
    let path_val = args[0];
    let data_val = args[1];

    if path_val.as_str().is_none() {
        return Value::number(0.0);
    }

    let mut is_src_file = false;
    let mut data_str = String::new();

    if data_val.is_object() {
        let ptr = data_val.as_gc_ptr();
        unsafe {
            match &(*ptr).data {
                crate::vm::gc::GcData::Struct(s) => {
                    if s.descriptor.name.as_ref() == "File" {
                        is_src_file = true;
                        let name_key = crate::vm::gc::get_or_create_string("name");
                        if let Some(name_val) = s.get_field(Value::string(name_key)) {
                            if let Some(s) = name_val.as_str() {
                                data_str = s.to_string();
                            }
                        }
                    }
                }
                crate::vm::gc::GcData::Object(map) => {
                    let is_file_key = crate::vm::gc::get_or_create_string("_isFile");
                    let is_file_val = map.get(&crate::vm::value::MapKey(Value::string(is_file_key)))
                        .map(|v| v.as_boolean())
                        .unwrap_or(false);
                    if is_file_val {
                        is_src_file = true;
                        let name_key = crate::vm::gc::get_or_create_string("name");
                        let name_val = map.get(&crate::vm::value::MapKey(Value::string(name_key))).cloned().unwrap_or(Value::null());
                        if let Some(s) = name_val.as_str() {
                            data_str = s.to_string();
                        }
                    }
                }
                _ => {}
            }
        }
    }

    if !is_src_file {
        data_str = match data_val.as_str() {
            Some(s) => s.to_string(),
            None => return Value::number(0.0),
        };
    }

    let args_to_pass = vec![
        path_val,
        Value::string(crate::vm::gc::get_or_create_string(&data_str)),
        Value::boolean(is_src_file)
    ];
    native_eronom_write_file(args_to_pass)
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

    // 2. Register global built-ins so they are available without imports
    vm.register_global("file", Value::native_function(native_file_global));
    vm.register_global("write", Value::native_function(native_write_global));

    // 3. Register native string helpers
    vm.register_global("stringSplit", Value::native_function(native_string_split));
    vm.register_global("stringIncludes", Value::native_function(native_string_includes));
    vm.register_global("stringStartsWith", Value::native_function(native_string_starts_with));
    vm.register_global("stringEndsWith", Value::native_function(native_string_ends_with));
    vm.register_global("stringSubstring", Value::native_function(native_string_substring));
    vm.register_global("stringReplace", Value::native_function(native_string_replace));
    vm.register_global("stringTrim", Value::native_function(native_string_trim));
    vm.register_global("stringLength", Value::native_function(native_string_length));
    vm.register_global("stringCharAt", Value::native_function(native_string_char_at));
    vm.register_global("stringIndexOf", Value::native_function(native_string_index_of));

    // 4. Register core structured concurrency runtime
    let preamble = r#"
        const callWithArity = (func, args) => {
            const length = arrayLen(args)
            if (length == 0) { return func() }
            if (length == 1) { return func(args[0]) }
            if (length == 2) { return func(args[0], args[1]) }
            if (length == 3) { return func(args[0], args[1], args[2]) }
            if (length == 4) { return func(args[0], args[1], args[2], args[3]) }
            if (length == 5) { return func(args[0], args[1], args[2], args[3], args[4]) }
            return func()
        }

        const spawnTask = (func, args) => {
            const pair = createPromisePair()
            setTimeout((f, a, resolve) => {
                const res = callWithArity(f, a)
                resolve(res)
            }, 0, func, args, pair.resolve)
            return pair.promise
        }

        const spawn = spawnTask
    "#;

    let tokens = crate::frontend::lex(preamble);
    let mut parser = crate::frontend::Parser::new(tokens);
    if let Ok(stmts) = parser.parse() {
        let compiler = crate::vm::compiler::Compiler::new();
        if let Ok(func) = compiler.compile(&stmts) {
            let _ = vm.run(func);
        }
    }

    Ok(())
}

fn native_string_split(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::null();
    }
    let s = match args[0].as_str() {
        Some(val) => val,
        None => return Value::null(),
    };
    let sep = match args[1].as_str() {
        Some(val) => val,
        None => return Value::null(),
    };
    let parts: Vec<Value> = s.split(sep)
        .map(|part| Value::string(get_or_create_string(part)))
        .collect();
    
    let ptr = gc_allocate(GcData::Array(parts));
    Value::array(ptr)
}

fn native_string_includes(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::boolean(false);
    }
    let s = match args[0].as_str() {
        Some(val) => val,
        None => return Value::boolean(false),
    };
    let search = match args[1].as_str() {
        Some(val) => val,
        None => return Value::boolean(false),
    };
    Value::boolean(s.contains(search))
}

fn native_string_starts_with(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::boolean(false);
    }
    let s = match args[0].as_str() {
        Some(val) => val,
        None => return Value::boolean(false),
    };
    let prefix = match args[1].as_str() {
        Some(val) => val,
        None => return Value::boolean(false),
    };
    Value::boolean(s.starts_with(prefix))
}

fn native_string_ends_with(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::boolean(false);
    }
    let s = match args[0].as_str() {
        Some(val) => val,
        None => return Value::boolean(false),
    };
    let suffix = match args[1].as_str() {
        Some(val) => val,
        None => return Value::boolean(false),
    };
    Value::boolean(s.ends_with(suffix))
}

fn native_string_substring(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::null();
    }
    let s = match args[0].as_str() {
        Some(val) => val,
        None => return Value::null(),
    };
    let start = args[1].as_number() as usize;
    let end = if args.len() >= 3 {
        args[2].as_number() as usize
    } else {
        s.len()
    };
    if start > s.len() || end > s.len() || start > end {
        return Value::null();
    }
    let sub = &s[start..end];
    let ptr = get_or_create_string(sub);
    Value::string(ptr)
}

fn native_string_replace(args: Vec<Value>) -> Value {
    if args.len() < 3 {
        return Value::null();
    }
    let s = match args[0].as_str() {
        Some(val) => val,
        None => return Value::null(),
    };
    let from = match args[1].as_str() {
        Some(val) => val,
        None => return Value::null(),
    };
    let to = match args[2].as_str() {
        Some(val) => val,
        None => return Value::null(),
    };
    let replaced = s.replace(from, to);
    let ptr = get_or_create_string(&replaced);
    Value::string(ptr)
}

fn native_string_trim(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let s = match args[0].as_str() {
        Some(val) => val,
        None => return Value::null(),
    };
    let trimmed = s.trim();
    let ptr = get_or_create_string(trimmed);
    Value::string(ptr)
}

fn native_string_length(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::number(0.0);
    }
    let s = match args[0].as_str() {
        Some(val) => val,
        None => return Value::number(0.0),
    };
    Value::number(s.len() as f64)
}

fn native_string_char_at(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::null();
    }
    let s = match args[0].as_str() {
        Some(val) => val,
        None => return Value::null(),
    };
    let idx = args[1].as_number() as usize;
    if let Some(c) = s.chars().nth(idx) {
        let mut buf = [0; 4];
        let c_str = c.encode_utf8(&mut buf);
        let ptr = get_or_create_string(c_str);
        Value::string(ptr)
    } else {
        Value::null()
    }
}

fn native_string_index_of(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::number(-1.0);
    }
    let s = match args[0].as_str() {
        Some(val) => val,
        None => return Value::number(-1.0),
    };
    let search = match args[1].as_str() {
        Some(val) => val,
        None => return Value::number(-1.0),
    };
    match s.find(search) {
        Some(idx) => Value::number(idx as f64),
        None => Value::number(-1.0),
    }
}

