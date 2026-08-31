use std::ffi::{c_char, c_void};

unsafe extern "C" {
    pub fn er_http_init_with_callbacks(
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
        ws_message_cb: extern "C" fn(*mut c_void, *const c_char, usize, *const c_char, usize, i32),
        ws_close_cb: extern "C" fn(*mut c_void, *const c_char, usize, i32, *const c_char, usize),
    );
    pub fn er_ws_register_route(path: *const c_char);
    pub fn er_ws_send(ws: *mut c_void, message: *const c_char, message_len: usize, is_binary: i32);
    pub fn er_http_listen_and_run(port: i32);
    
    pub fn er_http_response_write_status(res: *mut c_void, status_str: *const c_char, status_len: usize) -> bool;
    pub fn er_http_response_write_header(res: *mut c_void, key_str: *const c_char, key_len: usize, val_str: *const c_char, val_len: usize) -> bool;
    pub fn er_http_response_end(res: *mut c_void, data_str: *const c_char, data_len: usize) -> bool;
    
    pub fn er_http_create_timer(ms: i32, cb: extern "C" fn(*mut c_void));
}
