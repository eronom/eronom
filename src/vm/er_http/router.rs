use std::collections::HashMap;
use crate::vm::value::Value;
use crate::vm::gc::{gc_allocate, get_or_create_string, GcData};
use super::types::*;
use super::ws::*;
use super::static_files::native_static_route_handler;

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
        super::ffi::er_app_publish(
            topic_str.as_ptr() as *const std::ffi::c_char,
            topic_str.len(),
            bytes.as_ptr() as *const std::ffi::c_char,
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
        super::ffi::er_app_num_subscribers(topic_str.as_ptr() as *const std::ffi::c_char, topic_str.len())
    };
    Value::number(count as f64)
}

pub fn native_router_listen(args: Vec<Value>) -> Value {
    let mut port_val = Value::null();
    if !args.is_empty() {
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
    if !args.is_empty() {
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
    
    let path_str = match path_val.as_str() {
        Some(s) => s.to_string(),
        None => return Value::null(),
    };
    
    let open_name = get_or_create_string("open");
    let message_name = get_or_create_string("message");
    let close_name = get_or_create_string("close");
    
    let open_val = super::server::get_property_helper(callbacks_obj, Value::string(open_name));
    let message_val = super::server::get_property_helper(callbacks_obj, Value::string(message_name));
    let close_val = super::server::get_property_helper(callbacks_obj, Value::string(close_name));
    
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

pub fn register_route_internal(method: &str, args: Vec<Value>) -> Value {
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
    
    let mut path_str = match path_val.as_str() {
        Some(s) => s.to_string(),
        None => return Value::null(),
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

pub fn match_route_path(pattern: &str, path: &str) -> Option<HashMap<String, String>> {
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
