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
    let has_http_routes = ROUTES.with(|r| !r.borrow().is_empty());
    let has_ws_routes = WS_ROUTES.with(|r| !r.borrow().is_empty());
    let has_listen = LISTEN_PORT.with(|p| p.get().is_some());
    if !has_http_routes && !has_ws_routes && !has_listen {
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
