use std::ffi::{c_char, c_void, CString};
use crate::vm::value::Value;
use crate::vm::gc::{gc_allocate, get_or_create_string, GcData};
use super::ffi::*;
use super::types::*;
use super::hmr::check_and_reload_script_if_needed;

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
        let s = val.as_str().map(|s| s.as_bytes().to_vec()).unwrap_or_default();
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

pub fn create_ws_object(_ws: *mut c_void) -> Value {
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
