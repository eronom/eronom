use std::collections::HashMap;
use std::ffi::{c_char, c_void, CString};
use std::path::Path;
use crate::vm::value::Value;
use crate::vm::execute::VM;
use crate::vm::gc::{get_or_create_string, GcData};
use super::ffi::*;
use super::types::*;
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

// ─── High-Performance ErServer Struct ───────────────────────────────────────

pub struct ErServer<const SSL: bool = false> {
    pub raw: *mut c_void,
    pub port: i32,
}

impl<const SSL: bool> ErServer<SSL> {
    pub fn new(rust_server: *mut c_void) -> Self {
        let raw = unsafe { er_server_create(if SSL { 1 } else { 0 }, rust_server) };
        Self { raw, port: 0 }
    }

    pub fn register_route(&self, method: &str, path: &str, route_id: u32) {
        let method_c = CString::new(method).unwrap();
        let path_c = CString::new(path).unwrap();
        unsafe {
            er_server_register_route(self.raw, method_c.as_ptr(), path_c.as_ptr(), route_id);
        }
    }

    pub fn register_ws_route(&self, path: &str, route_id: u32) {
        let path_c = CString::new(path).unwrap();
        unsafe {
            er_server_register_ws_route(self.raw, path_c.as_ptr(), route_id);
        }
    }

    pub fn listen(&mut self, port: i32) -> bool {
        self.port = port;
        unsafe { er_server_listen(self.raw, port) }
    }

    pub fn run(&self) {
        unsafe { er_server_run(self.raw) }
    }
}

impl<const SSL: bool> Drop for ErServer<SSL> {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe { er_server_destroy(self.raw) };
            self.raw = std::ptr::null_mut();
        }
    }
}

// ─── Server Startup ──────────────────────────────────────────────────────────

pub fn start_http_server_if_needed(vm: &mut VM) {
    let has_serve = ACTIVE_SERVERS.with(|s| !s.borrow().is_empty());
    if has_serve {
        let active_server_raw = ACTIVE_SERVERS.with(|s| {
            s.borrow().values().find(|state| state.is_running && !state.raw.is_null()).map(|state| state.raw)
        });
        if let Some(raw) = active_server_raw {
            ACTIVE_VM.with(|active| {
                active.set(vm as *mut VM);
            });
            unsafe {
                er_server_run(raw);
            }
            ACTIVE_VM.with(|active| {
                active.set(std::ptr::null_mut());
            });
        }
        return;
    }

    let has_http_routes = ROUTES.with(|r| !r.borrow().is_empty());
    let has_ws_routes = WS_ROUTES.with(|r| !r.borrow().is_empty());
    if !has_http_routes && !has_ws_routes {
        return;
    }
    
    let requested_port = LISTEN_PORT.with(|p| p.get()).unwrap_or_else(|| get_port_from_config(vm));
    let port = if requested_port > 0 {
        crate::server::find_available_port(requested_port as u16) as i32
    } else {
        requested_port
    };
    if port != requested_port {
        println!("Port {} is already in use, trying port {} instead.", requested_port, port);
        LISTEN_PORT.with(|p| p.set(Some(port)));
    }
    println!("[HTTP] Starting uWebSockets HTTP server on port {}...", port);

    let mut server = ErServer::<false>::new(std::ptr::null_mut());

    ROUTES.with(|routes| {
        for (i, route) in routes.borrow().iter().enumerate() {
            server.register_route(&route.method, &route.path, i as u32);
        }
    });

    WS_ROUTES.with(|routes| {
        for (i, route) in routes.borrow().iter().enumerate() {
            server.register_ws_route(&route.path, i as u32);
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
        // Off-hot-path timer runs every 100ms for event loop and async HMR
        er_http_create_timer(100, er_http_on_timer);
    }

    if server.listen(port) {
        server.run();
    }

    ACTIVE_VM.with(|active| {
        active.set(std::ptr::null_mut());
    });
}

// ─── Asynchronous Off-Hot-Path Timer Callback ────────────────────────────────

#[unsafe(no_mangle)]
pub extern "C" fn er_http_on_timer(_timer: *mut c_void) {
    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    if !vm_ptr.is_null() {
        let vm = unsafe { &mut *vm_ptr };
        // Check file change asynchronously on timer tick (completely off HTTP request path!)
        check_and_reload_script_if_needed(vm);
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
pub extern "C" fn er_http_on_listening_instance(_rust_server: *mut c_void, _port: i32) {
    er_http_on_listening();
}

// ─── Direct Route Dispatch Callback (O(1) Route Matching) ───────────────────

#[unsafe(no_mangle)]
pub extern "C" fn er_http_on_route(
    _rust_server: *mut c_void,
    route_id: u32,
    res: *mut c_void,
    req_ptr: *mut c_void,
    url_ptr: *const c_char,
    url_len: usize,
    body_ptr: *const c_char,
    body_len: usize,
) {
    let raw_url = unsafe {
        if url_ptr.is_null() || url_len == 0 {
            ""
        } else {
            let slice = std::slice::from_raw_parts(url_ptr as *const u8, url_len);
            std::str::from_utf8(slice).unwrap_or("")
        }
    };
    let body_bytes = unsafe {
        if body_ptr.is_null() || body_len == 0 {
            &[]
        } else {
            std::slice::from_raw_parts(body_ptr as *const u8, body_len)
        }
    };

    // 1. Direct O(1) route callback lookup
    let route_opt = ROUTES.with(|routes| {
        routes.borrow().get(route_id as usize).cloned()
    });

    let Some(route) = route_opt else {
        return;
    };

    let (clean_path, raw_query) = match raw_url.find('?') {
        Some(idx) => (&raw_url[..idx], &raw_url[idx + 1..]),
        None => (raw_url, ""),
    };

    // 2. Extract route parameters (using uWS getParameter with fallback)
    let mut extracted_params = HashMap::new();
    if !route.param_names.is_empty() {
        for (i, name) in route.param_names.iter().enumerate() {
            let mut out_p: *const c_char = std::ptr::null();
            let mut out_len: usize = 0;
            let found = unsafe { er_http_req_get_parameter(req_ptr, i as u16, &mut out_p, &mut out_len) };
            if found && !out_p.is_null() && out_len > 0 {
                let p_slice = unsafe { std::slice::from_raw_parts(out_p as *const u8, out_len) };
                if let Ok(p_str) = std::str::from_utf8(p_slice) {
                    extracted_params.insert(name.clone(), p_str.to_string());
                }
            }
        }
        if extracted_params.is_empty() {
            if let Some(params) = match_route_path(&route.path, clean_path) {
                extracted_params = params;
            }
        }
    }

    ACTIVE_REQUEST_PATH.with(|p| *p.borrow_mut() = clean_path.to_string());
    ACTIVE_REQUEST_RAW_QUERY.with(|q| *q.borrow_mut() = raw_query.to_string());
    ACTIVE_REQUEST_METHOD.with(|m| *m.borrow_mut() = route.method.clone());
    ACTIVE_REQUEST_PARAMS.with(|p| *p.borrow_mut() = extracted_params.clone());
    ACTIVE_REQ_HANDLE.set(req_ptr);
    ACTIVE_HTTP_RESPONSE.set(res);
    ACTIVE_RESPONSE_STATE.with(|s| s.borrow_mut().reset());
    ACTIVE_REQUEST_HEADERS.with(|h| h.borrow_mut().clear());
    ACTIVE_REQUEST_COOKIES.with(|c| c.borrow_mut().clear());
    ACTIVE_REQUEST_QUERY.with(|q| q.borrow_mut().clear());

    ACTIVE_VM.with(|active| {
        let vm_ptr = active.get();
        if !vm_ptr.is_null() {
            let vm = unsafe { &mut *vm_ptr };

            let c_val = build_request_context(
                vm,
                raw_url,
                clean_path,
                raw_query,
                &route.method,
                req_ptr,
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
                if let Err(e) = vm.call_function_reentrant(route.callback, vec![c_val]) {
                    eprintln!("[HTTP] Error executing callback: {}", e);
                }
            }

            if let Err(e) = vm.run_event_loop() {
                eprintln!("[HTTP] Event loop error: {}", e);
            }
        }
    });

    ACTIVE_HTTP_RESPONSE.set(std::ptr::null_mut());
    ACTIVE_REQ_HANDLE.set(std::ptr::null_mut());
}

// ─── Fallback & Static File Dispatch Callback ───────────────────────────────

#[unsafe(no_mangle)]
pub extern "C" fn er_http_on_fallback(
    _rust_server: *mut c_void,
    res: *mut c_void,
    req_ptr: *mut c_void,
    method_ptr: *const c_char,
    method_len: usize,
    url_ptr: *const c_char,
    url_len: usize,
    body_ptr: *const c_char,
    body_len: usize,
) {
    let method = unsafe {
        let slice = std::slice::from_raw_parts(method_ptr as *const u8, method_len);
        std::str::from_utf8(slice).unwrap_or("")
    };
    let raw_url = unsafe {
        let slice = std::slice::from_raw_parts(url_ptr as *const u8, url_len);
        std::str::from_utf8(slice).unwrap_or("")
    };
    let body_bytes = unsafe {
        if body_ptr.is_null() || body_len == 0 {
            &[]
        } else {
            std::slice::from_raw_parts(body_ptr as *const u8, body_len)
        }
    };
    let (clean_path, raw_query) = match raw_url.find('?') {
        Some(idx) => (&raw_url[..idx], &raw_url[idx + 1..]),
        None => (raw_url, ""),
    };

    // Check if this request is handled by an active serve() instance:
    let server_port = _rust_server as usize as i32;
    let serve_state_opt = if server_port > 0 {
        ACTIVE_SERVERS.with(|s| s.borrow().get(&server_port).cloned())
    } else {
        ACTIVE_SERVERS.with(|s| s.borrow().values().find(|st| st.is_running).cloned())
    };

    if let Some(state) = serve_state_opt {
        ACTIVE_REQUEST_PATH.with(|p| *p.borrow_mut() = clean_path.to_string());
        ACTIVE_REQUEST_RAW_QUERY.with(|q| *q.borrow_mut() = raw_query.to_string());
        ACTIVE_REQUEST_METHOD.with(|m| *m.borrow_mut() = method.to_string());
        ACTIVE_REQ_HANDLE.set(req_ptr);
        ACTIVE_HTTP_RESPONSE.set(res);
        ACTIVE_RESPONSE_STATE.with(|s| s.borrow_mut().reset());

        ACTIVE_VM.with(|active| {
            let vm_ptr = active.get();
            if !vm_ptr.is_null() {
                let vm = unsafe { &mut *vm_ptr };
                let c_val = build_request_context(
                    vm,
                    raw_url,
                    clean_path,
                    raw_query,
                    method,
                    req_ptr,
                    HashMap::new(),
                    body_bytes,
                );

                let server_obj = state.server_val.unwrap_or(Value::null());
                let ret_result = vm.call_function_reentrant(state.fetch, vec![c_val, server_obj]);
                match ret_result {
                    Ok(ret_val) => {
                        let finished = ACTIVE_RESPONSE_STATE.with(|s| s.borrow().finished);
                        if !finished {
                            super::context::handle_fetch_return_value(res, ret_val);
                        }
                    }
                    Err(err) => {
                        eprintln!("[HTTP] Error in serve fetch: {}", err);
                        if let Some(err_cb) = state.error {
                            let err_str = get_or_create_string(&err);
                            if let Ok(err_ret) = vm.call_function_reentrant(err_cb, vec![Value::string(err_str), server_obj]) {
                                let finished = ACTIVE_RESPONSE_STATE.with(|s| s.borrow().finished);
                                if !finished {
                                    super::context::handle_fetch_return_value(res, err_ret);
                                }
                            }
                        } else {
                            let json_msg = serde_json::to_string(&err).unwrap_or_else(|_| "\"Internal Server Error\"".to_string());
                            let body = format!("{{\"error\":\"Internal Server Error\",\"message\":{}}}", json_msg);
                            super::context::flush_response(res, Some(body.as_bytes()), Some("application/json"), 500);
                        }
                    }
                }

                if let Err(e) = vm.run_event_loop() {
                    eprintln!("[HTTP] Event loop error: {}", e);
                }
            }
        });

        ACTIVE_HTTP_RESPONSE.set(std::ptr::null_mut());
        ACTIVE_REQ_HANDLE.set(std::ptr::null_mut());
        return;
    }

    // 1. Check if an ALL/wildcard route matches dynamically
    let mut extracted_params = HashMap::new();
    let callback_opt = ROUTER.with(|router| {
        if let Some((handler, params)) = router.borrow().find(method, clean_path) {
            extracted_params = params;
            Some(handler)
        } else {
            None
        }
    });

    if let Some(callback) = callback_opt {
        ACTIVE_REQUEST_PATH.with(|p| *p.borrow_mut() = clean_path.to_string());
        ACTIVE_REQUEST_RAW_QUERY.with(|q| *q.borrow_mut() = raw_query.to_string());
        ACTIVE_REQUEST_METHOD.with(|m| *m.borrow_mut() = method.to_string());
        ACTIVE_REQUEST_PARAMS.with(|p| *p.borrow_mut() = extracted_params.clone());
        ACTIVE_REQ_HANDLE.set(req_ptr);
        ACTIVE_HTTP_RESPONSE.set(res);
        ACTIVE_RESPONSE_STATE.with(|s| s.borrow_mut().reset());

        ACTIVE_VM.with(|active| {
            let vm_ptr = active.get();
            if !vm_ptr.is_null() {
                let vm = unsafe { &mut *vm_ptr };
                let c_val = build_request_context(
                    vm,
                    raw_url,
                    clean_path,
                    raw_query,
                    method,
                    req_ptr,
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
            }
        });

        ACTIVE_HTTP_RESPONSE.set(std::ptr::null_mut());
        ACTIVE_REQ_HANDLE.set(std::ptr::null_mut());
        return;
    }

    // 2. Static file serving fallback
    let req_path = if clean_path.starts_with('/') { &clean_path[1..] } else { clean_path };
    let mut headers_map = HashMap::new();
    if !req_ptr.is_null() {
        extern "C" fn on_hdr(ud: *mut c_void, k: *const c_char, k_len: usize, v: *const c_char, v_len: usize) {
            let map = unsafe { &mut *(ud as *mut HashMap<String, String>) };
            let k_slice = unsafe { std::slice::from_raw_parts(k as *const u8, k_len) };
            let v_slice = unsafe { std::slice::from_raw_parts(v as *const u8, v_len) };
            if let (Ok(k_str), Ok(v_str)) = (std::str::from_utf8(k_slice), std::str::from_utf8(v_slice)) {
                map.insert(k_str.to_ascii_lowercase(), v_str.to_string());
            }
        }
        unsafe {
            er_http_req_for_each_header(req_ptr, on_hdr, &mut headers_map as *mut _ as *mut c_void);
        }
    }

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

// ─── Legacy Wrapper Callback (Backward Compatibility) ───────────────────────

#[unsafe(no_mangle)]
pub extern "C" fn er_http_on_request(
    res: *mut c_void,
    method_ptr: *const c_char,
    method_len: usize,
    path_ptr: *const c_char,
    path_len: usize,
    _headers_ptr: *const c_char,
    _headers_len: usize,
    body_ptr: *const c_char,
    body_len: usize,
) {
    er_http_on_fallback(
        std::ptr::null_mut(),
        res,
        std::ptr::null_mut(),
        method_ptr,
        method_len,
        path_ptr,
        path_len,
        body_ptr,
        body_len,
    );
}

// ─── Native serve() Primitive ──────────────────────────────────────────────

pub fn native_serve(args: Vec<Value>) -> Value {
    let vm_ptr = ACTIVE_VM.with(|active| active.get());
    let (config_port, config_host) = if !vm_ptr.is_null() {
        let vm = unsafe { &*vm_ptr };
        (get_port_from_config(vm), "0.0.0.0".to_string())
    } else {
        (3000, "0.0.0.0".to_string())
    };

    let mut port = config_port;
    let mut hostname = config_host;
    let mut fetch_val = Value::null();
    let mut ws_val: Option<WsRoute> = None;
    let mut error_val: Option<Value> = None;

    if !args.is_empty() && args[0].is_object() {
        let opts = args[0];
        let port_key = get_or_create_string("port");
        let port_prop = get_property_helper(opts, Value::string(port_key));
        if port_prop.is_number() {
            port = port_prop.as_number() as i32;
        }

        let host_key = get_or_create_string("hostname");
        let host_prop = get_property_helper(opts, Value::string(host_key));
        if let Some(h) = host_prop.as_str() {
            hostname = h.to_string();
        }

        let fetch_key = get_or_create_string("fetch");
        let fetch_prop = get_property_helper(opts, Value::string(fetch_key));
        if fetch_prop.is_function() || fetch_prop.is_native_function() {
            fetch_val = fetch_prop;
        }

        let err_key = get_or_create_string("error");
        let err_prop = get_property_helper(opts, Value::string(err_key));
        if err_prop.is_function() || err_prop.is_native_function() {
            error_val = Some(err_prop);
        }

        let ws_key = get_or_create_string("websocket");
        let ws_prop = get_property_helper(opts, Value::string(ws_key));
        if ws_prop.is_object() {
            let open_key = get_or_create_string("open");
            let msg_key = get_or_create_string("message");
            let close_key = get_or_create_string("close");

            let open = {
                let v = get_property_helper(ws_prop, Value::string(open_key));
                if v.is_function() || v.is_native_function() { Some(v) } else { None }
            };
            let message = {
                let v = get_property_helper(ws_prop, Value::string(msg_key));
                if v.is_function() || v.is_native_function() { Some(v) } else { None }
            };
            let close = {
                let v = get_property_helper(ws_prop, Value::string(close_key));
                if v.is_function() || v.is_native_function() { Some(v) } else { None }
            };

            ws_val = Some(WsRoute {
                path: "/*".to_string(),
                open,
                message,
                close,
            });
        }
    }

    // 1. Hot reload check: If server already listening on this port, update and return!
    let existing_server = ACTIVE_SERVERS.with(|servers| {
        servers.borrow().get(&port).cloned()
    });
    if let Some(mut state) = existing_server {
        state.fetch = fetch_val;
        state.websocket = ws_val;
        state.error = error_val;
        let srv_obj = state.server_val.unwrap_or(Value::null());
        ACTIVE_SERVERS.with(|servers| {
            servers.borrow_mut().insert(port, state);
        });
        println!("[HTTP] Hot-reloaded server on port {}...", port);
        return srv_obj;
    }

    // 2. Determine final port
    let final_port = if port > 0 {
        crate::server::find_available_port(port as u16) as i32
    } else {
        3000
    };
    if final_port != port {
        println!("Port {} is in use, listening on port {} instead.", port, final_port);
    }

    // 3. Create ErServer instance passing final_port as user pointer
    let mut server = ErServer::<false>::new(final_port as usize as *mut c_void);

    if ws_val.is_some() {
        server.register_ws_route("/*", 0);
    }

    if !server.listen(final_port) {
        eprintln!("[HTTP] Failed to listen on port {}", final_port);
        return Value::null();
    }

    println!("[HTTP] Server listening on http://{}:{}", hostname, final_port);

    // 4. Construct JS Server object
    let mut server_map = crate::vm::gc::get_pooled_map(8);
    let port_str = get_or_create_string("port");
    let host_str = get_or_create_string("hostname");
    let stop_str = get_or_create_string("stop");
    let reload_str = get_or_create_string("reload");
    let publish_str = get_or_create_string("publish");
    let num_subs_str = get_or_create_string("numSubscribers");
    let upgrade_str = get_or_create_string("upgrade");
    let fetch_str = get_or_create_string("fetch");

    server_map.insert(crate::vm::value::MapKey(Value::string(port_str)), Value::number(final_port as f64));
    server_map.insert(crate::vm::value::MapKey(Value::string(host_str)), Value::string(get_or_create_string(&hostname)));
    server_map.insert(crate::vm::value::MapKey(Value::string(stop_str)), Value::native_function(native_server_stop));
    server_map.insert(crate::vm::value::MapKey(Value::string(reload_str)), Value::native_function(native_server_reload));
    server_map.insert(crate::vm::value::MapKey(Value::string(publish_str)), Value::native_function(super::router::native_router_publish));
    server_map.insert(crate::vm::value::MapKey(Value::string(num_subs_str)), Value::native_function(super::router::native_router_num_subscribers));
    server_map.insert(crate::vm::value::MapKey(Value::string(upgrade_str)), Value::native_function(native_server_upgrade));
    server_map.insert(crate::vm::value::MapKey(Value::string(fetch_str)), fetch_val);

    let server_obj = Value::object(crate::vm::gc::gc_allocate(GcData::Object(server_map)));

    // 5. Store server state and prevent server Drop from closing listen socket
    let raw_server = server.raw;
    std::mem::forget(server);

    ACTIVE_SERVERS.with(|servers| {
        servers.borrow_mut().insert(final_port, ServerState {
            raw: raw_server,
            port: final_port,
            hostname,
            fetch: fetch_val,
            websocket: ws_val,
            error: error_val,
            is_running: true,
            server_val: Some(server_obj),
        });
    });

    // 6. GC Roots
    crate::vm::gc::GC_ROOTS.with(|roots| {
        roots.borrow_mut().push(Box::new(|| {
            ACTIVE_SERVERS.with(|s| {
                for state in s.borrow().values() {
                    crate::vm::gc::mark_value(&state.fetch);
                    if let Some(err_cb) = &state.error {
                        crate::vm::gc::mark_value(err_cb);
                    }
                    if let Some(ws) = &state.websocket {
                        if let Some(open) = &ws.open { crate::vm::gc::mark_value(open); }
                        if let Some(msg) = &ws.message { crate::vm::gc::mark_value(msg); }
                        if let Some(close) = &ws.close { crate::vm::gc::mark_value(close); }
                    }
                    if let Some(srv) = &state.server_val {
                        crate::vm::gc::mark_value(srv);
                    }
                }
            });
        }));
    });

    // 7. Off-hot-path timer
    unsafe {
        er_http_create_timer(100, er_http_on_timer);
    }
    SERVER_RUNNING.with(|r| r.set(true));
    LISTEN_PORT.with(|p| p.set(Some(final_port)));

    server_obj
}

pub fn native_server_stop(args: Vec<Value>) -> Value {
    let target_port = if !args.is_empty() && args[0].is_number() {
        Some(args[0].as_number() as i32)
    } else {
        None
    };

    ACTIVE_SERVERS.with(|servers| {
        let mut s = servers.borrow_mut();
        if let Some(p) = target_port {
            if let Some(state) = s.get_mut(&p) {
                if !state.raw.is_null() {
                    unsafe { er_server_stop(state.raw) };
                    state.is_running = false;
                }
            }
        } else {
            for state in s.values_mut() {
                if !state.raw.is_null() {
                    unsafe { er_server_stop(state.raw) };
                    state.is_running = false;
                }
            }
        }
    });
    SERVER_RUNNING.with(|r| r.set(false));
    LISTEN_PORT.with(|p| p.set(None));
    Value::null()
}

pub fn native_server_reload(args: Vec<Value>) -> Value {
    if args.is_empty() || !args[0].is_object() {
        return Value::null();
    }
    let opts = args[0];
    let fetch_key = get_or_create_string("fetch");
    let fetch_prop = get_property_helper(opts, Value::string(fetch_key));

    ACTIVE_SERVERS.with(|servers| {
        let mut s = servers.borrow_mut();
        for state in s.values_mut() {
            if fetch_prop.is_function() || fetch_prop.is_native_function() {
                state.fetch = fetch_prop;
            }
        }
    });
    Value::boolean(true)
}

pub fn native_server_upgrade(_args: Vec<Value>) -> Value {
    Value::boolean(true)
}

