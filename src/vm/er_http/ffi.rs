use std::ffi::{c_char, c_void};

#[allow(dead_code)]
unsafe extern "C" {
    pub fn er_http_init();
    pub fn er_http_register_route(method: *const c_char, path: *const c_char);
    pub fn er_http_listen_and_run(port: i32);
    pub fn er_http_response_end_json(res: *mut c_void, json_str: *const c_char, json_len: usize) -> bool;
    pub fn er_http_response_end_html(res: *mut c_void, html_str: *const c_char, html_len: usize) -> bool;
    pub fn er_http_response_write_status(res: *mut c_void, status_str: *const c_char, status_len: usize) -> bool;
    pub fn er_http_response_write_header(res: *mut c_void, key_str: *const c_char, key_len: usize, val_str: *const c_char, val_len: usize) -> bool;
    pub fn er_http_response_write(res: *mut c_void, data_str: *const c_char, data_len: usize) -> bool;
    pub fn er_http_response_get_buffered_amount(res: *mut c_void) -> usize;
    pub fn er_http_response_end(res: *mut c_void, data_str: *const c_char, data_len: usize) -> bool;
    pub fn er_http_response_is_alive(res: *mut c_void) -> bool;
    pub fn er_http_response_release(res: *mut c_void);
    pub fn er_ws_get_buffered_amount(ws: *mut c_void) -> usize;
    
    pub fn er_ws_register_route(path: *const c_char);
    pub fn er_ws_send(ws: *mut c_void, message: *const c_char, message_len: usize, is_binary: i32);
    pub fn er_ws_close(ws: *mut c_void);
    pub fn er_ws_close_with_code(ws: *mut c_void, code: i32, message: *const c_char, message_len: usize);
    pub fn er_ws_subscribe(ws: *mut c_void, topic: *const c_char, topic_len: usize) -> bool;
    pub fn er_ws_unsubscribe(ws: *mut c_void, topic: *const c_char, topic_len: usize) -> bool;
    pub fn er_ws_is_subscribed(ws: *mut c_void, topic: *const c_char, topic_len: usize) -> bool;
    pub fn er_ws_publish(ws: *mut c_void, topic: *const c_char, topic_len: usize, message: *const c_char, message_len: usize, is_binary: i32) -> bool;
    pub fn er_app_publish(topic: *const c_char, topic_len: usize, message: *const c_char, message_len: usize, is_binary: i32) -> bool;
    pub fn er_app_num_subscribers(topic: *const c_char, topic_len: usize) -> u32;

    pub fn er_http_create_timer(ms: i32, cb: extern "C" fn(*mut c_void));
}
