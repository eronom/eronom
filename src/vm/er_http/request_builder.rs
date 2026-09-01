use std::collections::HashMap;
use crate::vm::value::Value;
use crate::vm::execute::VM;
use crate::vm::gc::{get_or_create_string, GcData};
use super::context::*;
use super::multipart::{construct_file_object, parse_header_params, parse_multipart};

pub fn build_request_context(
    vm: &mut VM,
    path: &str,
    clean_path: &str,
    raw_query: &str,
    method: &str,
    headers_map: &HashMap<String, String>,
    parsed_query: &HashMap<String, String>,
    parsed_cookies: &HashMap<String, String>,
    extracted_params: HashMap<String, String>,
    body_bytes: &[u8],
) -> Value {
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
    for (k, v) in parsed_query {
        let k_str = get_or_create_string(k);
        let v_str = get_or_create_string(v);
        query_obj_map.insert(crate::vm::value::MapKey(Value::string(k_str)), Value::string(v_str));
    }
    let query_obj = Value::object(crate::vm::gc::gc_allocate(GcData::Object(query_obj_map)));

    // Headers map
    let mut headers_obj_map = crate::vm::gc::get_pooled_map(headers_map.len());
    for (k, v) in headers_map {
        let k_str = get_or_create_string(k);
        let v_str = get_or_create_string(v);
        headers_obj_map.insert(crate::vm::value::MapKey(Value::string(k_str)), Value::string(v_str));
    }
    let headers_obj = Value::object(crate::vm::gc::gc_allocate(GcData::Object(headers_obj_map)));

    // Cookies map
    let mut cookies_obj_map = crate::vm::gc::get_pooled_map(parsed_cookies.len());
    for (k, v) in parsed_cookies {
        let k_str = get_or_create_string(k);
        let v_str = get_or_create_string(v);
        cookies_obj_map.insert(crate::vm::value::MapKey(Value::string(k_str)), Value::string(v_str));
    }
    let cookies_obj = Value::object(crate::vm::gc::gc_allocate(GcData::Object(cookies_obj_map)));

    // Multipart files parsing
    let mut content_type = String::new();
    for (k, v) in headers_map {
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

    Value::object(crate::vm::gc::gc_allocate(GcData::Object(map)))
}
