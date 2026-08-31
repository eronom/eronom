use std::collections::HashMap;
use std::ffi::{c_char, c_void, CString};
use std::fs;
use crate::compiler;
use super::types::*;
use super::ffi::*;
use super::router::resolve_path;
use super::render::execute_api_route;

pub extern "C" fn dev_http_callback(
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
    let method = unsafe {
        let slice = std::slice::from_raw_parts(method_ptr as *const u8, method_len);
        std::str::from_utf8(slice).unwrap_or("")
    };
    let path = unsafe {
        let slice = std::slice::from_raw_parts(path_ptr as *const u8, path_len);
        std::str::from_utf8(slice).unwrap_or("")
    };
    let headers = unsafe {
        let slice = std::slice::from_raw_parts(headers_ptr as *const u8, headers_len);
        std::str::from_utf8(slice).unwrap_or("")
    };
    let body = unsafe {
        if body_ptr.is_null() {
            &[]
        } else {
            std::slice::from_raw_parts(body_ptr as *const u8, body_len)
        }
    };

    if let Err(e) = handle_dev_request(res, method, path, headers, body) {
        let err_msg = format!("Internal Server Error: {}", e);
        let err_bytes = err_msg.as_bytes();
        unsafe {
            let status = CString::new("500 Internal Server Error").unwrap();
            er_http_response_write_status(res, status.as_ptr(), status.as_bytes().len());
            let content_type = CString::new("text/plain; charset=utf-8").unwrap();
            let content_type_key = CString::new("Content-Type").unwrap();
            er_http_response_write_header(res, content_type_key.as_ptr(), content_type_key.as_bytes().len(), content_type.as_ptr(), content_type.as_bytes().len());
            er_http_response_end(res, err_msg.as_ptr() as *const c_char, err_bytes.len());
        }
    }
}

pub extern "C" fn dev_ws_open_callback(
    ws: *mut c_void,
    _path_ptr: *const c_char,
    _path_len: usize,
) {
    let mut conns = ACTIVE_CONNECTIONS.lock().unwrap();
    if !conns.contains(&(ws as usize)) {
        conns.push(ws as usize);
    }
}

pub extern "C" fn dev_ws_message_callback(
    _ws: *mut c_void,
    _path_ptr: *const c_char,
    _path_len: usize,
    _msg_ptr: *const c_char,
    _msg_len: usize,
    _is_binary: i32,
) {
}

pub extern "C" fn dev_ws_close_callback(
    ws: *mut c_void,
    _path_ptr: *const c_char,
    _path_len: usize,
    _code: i32,
    _msg_ptr: *const c_char,
    _msg_len: usize,
) {
    let mut conns = ACTIVE_CONNECTIONS.lock().unwrap();
    if let Some(pos) = conns.iter().position(|&x| x == ws as usize) {
        conns.remove(pos);
    }
}

pub extern "C" fn check_hmr_queue(_timer: *mut c_void) {
    let mut queue = HMR_QUEUE.lock().unwrap();
    if !queue.is_empty() {
        let conns = ACTIVE_CONNECTIONS.lock().unwrap();
        for &ws in conns.iter() {
            for msg in queue.iter() {
                let msg_c = CString::new(msg.as_str()).unwrap();
                unsafe {
                    er_ws_send(ws as *mut c_void, msg_c.as_ptr(), msg_c.as_bytes().len(), 0);
                }
            }
        }
        queue.clear();
    }
}

pub fn handle_dev_request(res: *mut c_void, method: &str, target: &str, headers: &str, body: &[u8]) -> anyhow::Result<()> {
    let base_path = BASE_PATH.lock().unwrap().clone().unwrap();
    let default_file = DEFAULT_FILE.lock().unwrap().clone();
    let is_prod = *IS_PROD.lock().unwrap();

    println!("Request: {} {}", method, target);

    if target.starts_with("/__erm_src/") {
        let rel_file = &target["/__erm_src/".len()..];
        let file_path = base_path.join(rel_file);
        if file_path.exists() && file_path.is_file() {
            if let Ok(src) = fs::read_to_string(&file_path) {
                unsafe {
                    let status = CString::new("200 OK").unwrap();
                    er_http_response_write_status(res, status.as_ptr(), status.as_bytes().len());
                    let content_type = CString::new("text/plain; charset=utf-8").unwrap();
                    let content_type_key = CString::new("Content-Type").unwrap();
                    er_http_response_write_header(res, content_type_key.as_ptr(), content_type_key.as_bytes().len(), content_type.as_ptr(), content_type.as_bytes().len());
                    er_http_response_end(res, src.as_ptr() as *const c_char, src.len());
                }
                return Ok(());
            }
        }
    }

    let app_dir = if base_path.join("app").exists() {
        base_path.join("app")
    } else {
        base_path.clone()
    };

    let mut params = HashMap::new();
    let file_path = if target == "/" {
        if let Some(ref def_file) = default_file {
            def_file.clone()
        } else {
            let index_erm = app_dir.join("pages").join("index.erm");
            if index_erm.exists() {
                index_erm
            } else {
                app_dir.join("pages").join("index.html")
            }
        }
    } else {
        let resolve_root = if !target.starts_with("/api/") {
            app_dir.join("pages")
        } else {
            base_path.join("server")
        };

        if let Some((path, p)) = resolve_path(&resolve_root, target) {
            params = p;
            path
        } else {
            let primary_fallback = resolve_root.join(&target[1..]);
            if !target.starts_with("/api/") && !primary_fallback.exists() {
                let secondary_fallback = app_dir.join(&target[1..]);
                if secondary_fallback.exists() {
                    secondary_fallback
                } else {
                    let base_fallback = base_path.join(&target[1..]);
                    if base_fallback.exists() {
                        base_fallback
                    } else {
                        primary_fallback
                    }
                }
            } else {
                primary_fallback
            }
        }
    };

    let mut file_path = file_path;
    if !file_path.exists() && default_file.is_some() {
        file_path = default_file.clone().unwrap();
    }

    if file_path.exists() && file_path.is_file() {
        if file_path.extension().map_or(false, |ext| ext == "erm") {
            let render_result = {
                let content = fs::read_to_string(&file_path)?;
                compiler::process_erm_component(file_path.to_str().unwrap(), &content, is_prod, &params)
            };

            match render_result {
                Ok(processed) => {
                    let rel_path = file_path.strip_prefix(&base_path)
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_else(|_| file_path.to_string_lossy().into_owned());
                    let mut processed = processed;
                    if !is_prod {
                        let filename_script = format!("<script>window.__erm_filename = \"{}\";</script>\n", rel_path);
                        if let Some(pos) = processed.find("<head>") {
                            processed.insert_str(pos + 6, &filename_script);
                        } else {
                            processed.insert_str(0, &filename_script);
                        }
                    }
                    unsafe {
                        let status = CString::new("200 OK").unwrap();
                        er_http_response_write_status(res, status.as_ptr(), status.as_bytes().len());
                        let content_type = CString::new("text/html; charset=utf-8").unwrap();
                        let content_type_key = CString::new("Content-Type").unwrap();
                        er_http_response_write_header(res, content_type_key.as_ptr(), content_type_key.as_bytes().len(), content_type.as_ptr(), content_type.as_bytes().len());
                        er_http_response_end(res, processed.as_ptr() as *const c_char, processed.len());
                    }
                }
                Err(e) => {
                    let rel_path = file_path.strip_prefix(&base_path)
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_else(|_| file_path.to_string_lossy().into_owned());

                    let escape_html = |s: &str| -> String {
                        s.replace('&', "&amp;")
                         .replace('<', "&lt;")
                         .replace('>', "&gt;")
                         .replace('"', "&quot;")
                         .replace('\'', "&#39;")
                    };

                    let err_msg_escaped = escape_html(&format!("{}", e));
                    let rel_path_escaped = escape_html(&rel_path);

                    let html_content = format!(r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>Compiler Error</title>
  <style>
    body {{
      background-color: #0b0b0d;
      margin: 0;
      padding: 0;
      min-height: 100vh;
    }}
  </style>
  <script src="/modules/erm/hmr.js"></script>
</head>
<body data-compile-error-file="{}" data-compile-error-message="{}">
</body>
</html>"#, rel_path_escaped, err_msg_escaped);

                    unsafe {
                        let status = CString::new("500 Internal Server Error").unwrap();
                        er_http_response_write_status(res, status.as_ptr(), status.as_bytes().len());
                        let content_type = CString::new("text/html; charset=utf-8").unwrap();
                        let content_type_key = CString::new("Content-Type").unwrap();
                        er_http_response_write_header(
                            res,
                            content_type_key.as_ptr(),
                            content_type_key.as_bytes().len(),
                            content_type.as_ptr(),
                            content_type.as_bytes().len(),
                        );
                        er_http_response_end(res, html_content.as_ptr() as *const c_char, html_content.len());
                    }
                }
            }
        } else if file_path.extension().map_or(false, |ext| ext == "er") {
            if let Err(e) = execute_api_route(res, method, target, headers, body, &file_path) {
                let err_msg = format!("Error running route: {}", e);
                unsafe {
                    let status = CString::new("500 Internal Server Error").unwrap();
                    er_http_response_write_status(res, status.as_ptr(), status.as_bytes().len());
                    er_http_response_end(res, err_msg.as_ptr() as *const c_char, err_msg.len());
                }
            }
        } else {
            let content = fs::read(&file_path)?;
            let mime = if let Some(ext) = file_path.extension() {
                let ext_str = ext.to_string_lossy().to_lowercase();
                match ext_str.as_str() {
                    "html" => Some("text/html; charset=utf-8"),
                    "css" => Some("text/css; charset=utf-8"),
                    "js" => Some("application/javascript; charset=utf-8"),
                    "json" => Some("application/json; charset=utf-8"),
                    "png" => Some("image/png"),
                    "jpg" | "jpeg" => Some("image/jpeg"),
                    "gif" => Some("image/gif"),
                    "svg" => Some("image/svg+xml"),
                    "ico" => Some("image/x-icon"),
                    _ => None,
                }
            } else {
                None
            };
            unsafe {
                let status = CString::new("200 OK").unwrap();
                er_http_response_write_status(res, status.as_ptr(), status.as_bytes().len());
                if let Some(mime_type) = mime {
                    let content_type = CString::new(mime_type).unwrap();
                    let content_type_key = CString::new("Content-Type").unwrap();
                    er_http_response_write_header(res, content_type_key.as_ptr(), content_type_key.as_bytes().len(), content_type.as_ptr(), content_type.as_bytes().len());
                }
                er_http_response_end(res, content.as_ptr() as *const c_char, content.len());
            }
        }
    } else {
        unsafe {
            let status = CString::new("404 Not Found").unwrap();
            er_http_response_write_status(res, status.as_ptr(), status.as_bytes().len());
            let not_found = "Not Found";
            er_http_response_end(res, not_found.as_ptr() as *const c_char, not_found.len());
        }
    }
    Ok(())
}
