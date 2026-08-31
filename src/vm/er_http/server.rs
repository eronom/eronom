use std::collections::HashMap;
use std::ffi::{c_char, c_void, CString};
use std::path::Path;
use crate::vm::value::Value;
use crate::vm::execute::VM;
use crate::vm::gc::{get_or_create_string, GcData};
use super::ffi::*;
use super::types::*;
use super::utils::*;
use super::static_files::serve_static_file;
use super::hmr::check_and_reload_script_if_needed;
use super::router::match_route_path;
use super::request_builder::build_request_context;

pub fn get_property_helper(obj: Value, name_val: Value) -> Value {
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

pub fn get_port_from_config(vm: &VM) -> i32 {
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

pub fn end_http_response_json(res: *mut c_void, json: &str) {
    if res.is_null() {
        return;
    }
    unsafe {
        er_http_response_end_json(res, json.as_ptr() as *const c_char, json.len());
    }
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

                let c_val = build_request_context(
                    vm,
                    path,
                    clean_path,
                    raw_query,
                    method,
                    &headers_map,
                    &parsed_query,
                    &parsed_cookies,
                    extracted_params,
                    body_bytes,
                );

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
        let req_path = if clean_path.starts_with('/') { &clean_path[1..] } else { clean_path };
        let mut served = serve_static_file(res, Path::new(req_path), &headers_map);
        
        if !served && !req_path.starts_with("public/") {
            served = serve_static_file(res, Path::new(&format!("public/{}", req_path)), &headers_map);
        }
        if !served && !req_path.starts_with("build/") {
            served = serve_static_file(res, Path::new(&format!("build/{}", req_path)), &headers_map);
        }
        if !served && !req_path.starts_with("css/") {
            served = serve_static_file(res, Path::new(&format!("css/{}", req_path)), &headers_map);
        }
        if !served && clean_path.starts_with("/modules/") {
            served = serve_static_file(res, Path::new(&clean_path[1..]), &headers_map);
        }

        if !served {
            unsafe {
                let status = CString::new("404 Not Found").unwrap();
                er_http_response_write_status(res, status.as_ptr(), status.as_bytes().len());
                let c_str = CString::new("{\"error\": \"Not Found\"}").unwrap();
                er_http_response_end_json(res, c_str.as_ptr(), c_str.as_bytes().len());
            }
        }
    }
}
