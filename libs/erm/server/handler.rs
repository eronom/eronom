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
        eprintln!("[ERROR] Internal Server Error: {}", e);
        if *IS_PROD.lock().unwrap() {
            let html = r#"<!DOCTYPE html><html><head><meta charset="utf-8"><title>500 Internal Server Error</title><style>body{background:#0b0b0d;color:#fff;display:flex;align-items:center;justify-content:center;height:100vh;margin:0;font-family:system-ui,-apple-system,sans-serif;}h1{font-size:3rem;margin:0 0 0.5rem;}p{color:#888;margin:0;}</style></head><body><div style="text-align:center"><h1>500</h1><p>Internal Server Error</p></div></body></html>"#;
            send_response(res, "500 Internal Server Error", "text/html; charset=utf-8", html.as_bytes());
        } else {
            let err_msg = format!("Internal Server Error: {}", e);
            send_response(res, "500 Internal Server Error", "text/plain; charset=utf-8", err_msg.as_bytes());
        }
    }
}

pub fn extract_header<'a>(headers: &'a str, name: &str) -> Option<&'a str> {
    for line in headers.lines() {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case(name) {
                return Some(v.trim());
            }
        }
    }
    None
}

pub fn send_response_with_headers(
    res: *mut c_void,
    status: &str,
    content_type: &str,
    extra_headers: &[(&str, &str)],
    body: &[u8],
) {
    unsafe {
        let status_c = CString::new(status).unwrap_or_else(|_| CString::new("500 Internal Server Error").unwrap());
        er_http_response_write_status(res, status_c.as_ptr(), status_c.as_bytes().len());
        let ct_key = c"Content-Type";
        let ct_val = CString::new(content_type).unwrap_or_else(|_| CString::new("text/plain").unwrap());
        er_http_response_write_header(
            res,
            ct_key.as_ptr(),
            ct_key.to_bytes().len(),
            ct_val.as_ptr(),
            ct_val.as_bytes().len(),
        );
        for (k, v) in extra_headers {
            if let (Ok(k_c), Ok(v_c)) = (CString::new(*k), CString::new(*v)) {
                er_http_response_write_header(
                    res,
                    k_c.as_ptr(),
                    k_c.as_bytes().len(),
                    v_c.as_ptr(),
                    v_c.as_bytes().len(),
                );
            }
        }
        er_http_response_end(res, body.as_ptr() as *const c_char, body.len());
    }
}

pub fn send_not_modified(res: *mut c_void, etag: &str) {
    unsafe {
        let status = c"304 Not Modified";
        er_http_response_write_status(res, status.as_ptr(), status.to_bytes().len());
        let etag_key = c"ETag";
        let etag_val = CString::new(etag).unwrap_or_else(|_| CString::new("\"\"").unwrap());
        er_http_response_write_header(
            res,
            etag_key.as_ptr(),
            etag_key.to_bytes().len(),
            etag_val.as_ptr(),
            etag_val.as_bytes().len(),
        );
        er_http_response_end(res, std::ptr::null(), 0);
    }
}

pub fn send_response(res: *mut c_void, status: &str, content_type: &str, body: &[u8]) {
    send_response_with_headers(res, status, content_type, &[], body);
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
    let connected_msg = serde_json::json!({
        "type": "connected"
    }).to_string();
    let msg_c = CString::new(connected_msg).unwrap();
    unsafe {
        er_ws_send(ws, msg_c.as_ptr(), msg_c.as_bytes().len(), 0);
    }
}

pub extern "C" fn dev_ws_message_callback(
    ws: *mut c_void,
    _path_ptr: *const c_char,
    _path_len: usize,
    msg_ptr: *const c_char,
    msg_len: usize,
    _is_binary: i32,
) {
    if !msg_ptr.is_null() && msg_len > 0 {
        let msg_slice = unsafe { std::slice::from_raw_parts(msg_ptr as *const u8, msg_len) };
        if let Ok(msg_str) = std::str::from_utf8(msg_slice) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(msg_str) {
                if val.get("type").and_then(|t| t.as_str()) == Some("ping") {
                    let pong = serde_json::json!({ "type": "pong" }).to_string();
                    let msg_c = CString::new(pong).unwrap();
                    unsafe {
                        er_ws_send(ws, msg_c.as_ptr(), msg_c.as_bytes().len(), 0);
                    }
                }
            }
        }
    }
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

pub fn handle_dev_request(res: *mut c_void, method: &str, raw_target: &str, headers: &str, body: &[u8]) -> anyhow::Result<()> {
    let base_path = BASE_PATH.lock().unwrap().clone().unwrap();
    let default_file = DEFAULT_FILE.lock().unwrap().clone();
    let is_prod = *IS_PROD.lock().unwrap();

    let (target, query) = if let Some(pos) = raw_target.find('?') {
        (&raw_target[..pos], Some(&raw_target[pos + 1..]))
    } else {
        (raw_target, None)
    };

    // In production, check in-memory static asset cache first
    if is_prod {
        let cache = STATIC_ASSET_CACHE.lock().unwrap();
        if let Some(cached) = cache.get(target) {
            if let Some(client_etag) = extract_header(headers, "if-none-match") {
                let norm_client = client_etag.trim().trim_matches('"');
                let norm_server = cached.etag.trim().trim_matches('"');
                if norm_client == norm_server {
                    send_not_modified(res, &cached.etag);
                    return Ok(());
                }
            }

            let mut extra = Vec::new();
            extra.push(("ETag", cached.etag.as_str()));
            if cached.is_immutable {
                extra.push(("Cache-Control", "public, max-age=31536000, immutable"));
            } else {
                extra.push(("Cache-Control", "public, max-age=3600, must-revalidate"));
            }
            send_response_with_headers(res, "200 OK", cached.content_type, &extra, &cached.content);
            return Ok(());
        }
    }

    if target.starts_with("/__erm_src/") {
        if is_prod {
            send_response(res, "403 Forbidden", "text/plain; charset=utf-8", b"Forbidden");
            return Ok(());
        }
        let rel_file = &target["/__erm_src/".len()..];
        let file_path = base_path.join(rel_file);
        if file_path.exists() && file_path.is_file() {
            if let Ok(src) = fs::read_to_string(&file_path) {
                send_response(res, "200 OK", "text/plain; charset=utf-8", src.as_bytes());
                return Ok(());
            }
        }
    }

    if target == "/modules/erm/hmr.js" {
        let content: std::borrow::Cow<'static, [u8]> = if base_path.join("modules/erm/hmr.js").exists() {
            fs::read(base_path.join("modules/erm/hmr.js"))?.into()
        } else if base_path.join("core/hmr.js").exists() {
            fs::read(base_path.join("core/hmr.js"))?.into()
        } else {
            include_bytes!("../../init/modules/erm/hmr.js").as_slice().into()
        };

        send_response(res, "200 OK", "application/javascript; charset=utf-8", &content);
        return Ok(());
    }

    if target == "/modules/erm/runtime.js" {
        let content: std::borrow::Cow<'static, [u8]> = if base_path.join("modules/erm/runtime.js").exists() {
            fs::read(base_path.join("modules/erm/runtime.js"))?.into()
        } else if base_path.join("core/runtime.js").exists() {
            fs::read(base_path.join("core/runtime.js"))?.into()
        } else {
            include_bytes!("../../init/modules/erm/runtime.js").as_slice().into()
        };

        send_response(res, "200 OK", "application/javascript; charset=utf-8", &content);
        return Ok(());
    }

    let app_dir = if base_path.join("app").exists() {
        base_path.join("app")
    } else {
        base_path.clone()
    };

    let mut params = HashMap::new();
    if let Some(q) = query {
        for pair in q.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                params.insert(k.to_string(), v.to_string());
            }
        }
    }
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
                    } else if target == "/favicon.ico" {
                        let logo_app = app_dir.join("eronom_logo.png");
                        let logo_pages = app_dir.join("pages").join("eronom_logo.png");
                        let logo_base = base_path.join("eronom_logo.png");
                        if logo_app.exists() {
                            logo_app
                        } else if logo_pages.exists() {
                            logo_pages
                        } else if logo_base.exists() {
                            logo_base
                        } else {
                            primary_fallback
                        }
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
                    send_response(res, "200 OK", "text/html; charset=utf-8", processed.as_bytes());
                }
                Err(e) => {
                    eprintln!("[ERROR] Failed to compile {}: {}", file_path.display(), e);
                    if is_prod {
                        let html = r#"<!DOCTYPE html><html><head><meta charset="utf-8"><title>500 Internal Server Error</title><style>body{background:#0b0b0d;color:#fff;display:flex;align-items:center;justify-content:center;height:100vh;margin:0;font-family:system-ui,-apple-system,sans-serif;}h1{font-size:3rem;margin:0 0 0.5rem;}p{color:#888;margin:0;}</style></head><body><div style="text-align:center"><h1>500</h1><p>Internal Server Error</p></div></body></html>"#;
                        send_response(res, "500 Internal Server Error", "text/html; charset=utf-8", html.as_bytes());
                    } else {
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

                        send_response(res, "500 Internal Server Error", "text/html; charset=utf-8", html_content.as_bytes());
                    }
                }
            }
        } else if file_path.extension().map_or(false, |ext| ext == "er") {
            if let Err(e) = execute_api_route(res, method, target, headers, body, &file_path) {
                eprintln!("[ERROR] Route error on {}: {}", file_path.display(), e);
                let err_msg = if is_prod {
                    "Internal Server Error".to_string()
                } else {
                    format!("Error running route: {}", e)
                };
                unsafe {
                    let status = CString::new("500 Internal Server Error").unwrap();
                    er_http_response_write_status(res, status.as_ptr(), status.as_bytes().len());
                    er_http_response_end(res, err_msg.as_ptr() as *const c_char, err_msg.len());
                }
            }
        } else {
            let content = fs::read(&file_path)?;
            let mime = if let Some(ext) = file_path.extension() {
                let ext_str = ext.to_string_lossy();
                guess_mime_type(&ext_str)
            } else {
                "application/octet-stream"
            };

            if is_prod {
                let hash = crate::compiler::compute_asset_hash(&content);
                let etag = format!("\"{}\"", hash);
                if let Some(client_etag) = extract_header(headers, "if-none-match") {
                    let norm_client = client_etag.trim().trim_matches('"');
                    if norm_client == hash {
                        send_not_modified(res, &etag);
                        return Ok(());
                    }
                }
                let is_immutable = target.starts_with("/assets/");
                let cache_ctrl = if is_immutable {
                    "public, max-age=31536000, immutable"
                } else {
                    "public, max-age=3600, must-revalidate"
                };
                let extra = [
                    ("ETag", etag.as_str()),
                    ("Cache-Control", cache_ctrl),
                ];
                send_response_with_headers(res, "200 OK", mime, &extra, &content);
            } else {
                send_response(res, "200 OK", mime, &content);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_header_case_insensitive() {
        let headers = "Host: localhost:3000\r\nUser-Agent: curl/7.68.0\r\nIf-None-Match: \"12345678\"\r\nAccept: */*\r\n";
        assert_eq!(extract_header(headers, "if-none-match"), Some("\"12345678\""));
        assert_eq!(extract_header(headers, "IF-NONE-MATCH"), Some("\"12345678\""));
        assert_eq!(extract_header(headers, "host"), Some("localhost:3000"));
        assert_eq!(extract_header(headers, "non-existent"), None);
    }
}
