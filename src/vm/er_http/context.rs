use std::ffi::{c_char, c_void, CString};
use std::path::Path;
use crate::vm::value::Value;
use crate::vm::gc::{get_or_create_string, GcData};
use super::ffi::*;
use super::types::*;
use super::utils::{format_cookie, status_code_to_status_line, value_to_json};
use super::static_files::serve_static_file;

pub fn flush_response(
    res_ptr: *mut c_void,
    body: Option<&[u8]>,
    default_content_type: Option<&str>,
    default_status: u16,
) -> bool {
    if res_ptr.is_null() {
        return false;
    }

    let (status_code, headers, cookies, already_finished) = ACTIVE_RESPONSE_STATE.with(|state| {
        let mut s = state.borrow_mut();
        if s.finished {
            return (0, Vec::new(), Vec::new(), true);
        }
        s.finished = true;
        (
            s.status.unwrap_or(default_status),
            s.headers.clone(),
            s.cookies.clone(),
            false,
        )
    });

    if already_finished {
        return false;
    }

    unsafe {
        // 1. Status
        let status_line = status_code_to_status_line(status_code);
        er_http_response_write_status(res_ptr, status_line.as_ptr() as *const c_char, status_line.len());

        // 2. Content-Type
        let mut has_content_type = false;
        for (k, _) in &headers {
            if k.eq_ignore_ascii_case("content-type") {
                has_content_type = true;
                break;
            }
        }
        if !has_content_type {
            if let Some(ct) = default_content_type {
                let k = "Content-Type";
                er_http_response_write_header(res_ptr, k.as_ptr() as *const c_char, k.len(), ct.as_ptr() as *const c_char, ct.len());
            }
        }

        // 3. Headers
        for (k, v) in &headers {
            er_http_response_write_header(res_ptr, k.as_ptr() as *const c_char, k.len(), v.as_ptr() as *const c_char, v.len());
        }

        // 4. Cookies
        let set_cookie = "Set-Cookie";
        for cookie_str in &cookies {
            er_http_response_write_header(res_ptr, set_cookie.as_ptr() as *const c_char, set_cookie.len(), cookie_str.as_ptr() as *const c_char, cookie_str.len());
        }

        // 5. Body
        let body_bytes = body.unwrap_or(b"");
        er_http_response_end(res_ptr, body_bytes.as_ptr() as *const c_char, body_bytes.len());
    }

    true
}

pub fn native_context_json(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let res_ptr = ACTIVE_HTTP_RESPONSE.with(|resp| resp.get());
    if res_ptr.is_null() {
        return Value::null();
    }
    let data = args[0];
    if args.len() >= 2 && args[1].is_number() {
        ACTIVE_RESPONSE_STATE.with(|s| s.borrow_mut().status = Some(args[1].as_number() as u16));
    }
    let json_val = value_to_json(data);
    let json_str = serde_json::to_string(&json_val).unwrap_or_else(|_| "null".to_string());
    flush_response(res_ptr, Some(json_str.as_bytes()), Some("application/json"), 200);
    Value::null()
}

pub fn native_context_html(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let res_ptr = ACTIVE_HTTP_RESPONSE.with(|resp| resp.get());
    if res_ptr.is_null() {
        return Value::null();
    }
    let html_val = args[0];
    if args.len() >= 2 && args[1].is_number() {
        ACTIVE_RESPONSE_STATE.with(|s| s.borrow_mut().status = Some(args[1].as_number() as u16));
    }
    let html_str = if html_val.is_string() {
        match html_val.as_str() {
            Some(s) => s.to_string(),
            None => return Value::null(),
        }
    } else {
        html_val.to_string()
    };
    flush_response(res_ptr, Some(html_str.as_bytes()), Some("text/html; charset=utf-8"), 200);
    Value::null()
}

pub fn native_context_text(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let res_ptr = ACTIVE_HTTP_RESPONSE.with(|resp| resp.get());
    if res_ptr.is_null() {
        return Value::null();
    }
    let text_val = args[0];
    if args.len() >= 2 && args[1].is_number() {
        ACTIVE_RESPONSE_STATE.with(|s| s.borrow_mut().status = Some(args[1].as_number() as u16));
    }
    let text_str = if text_val.is_string() {
        match text_val.as_str() {
            Some(s) => s.to_string(),
            None => return Value::null(),
        }
    } else {
        text_val.to_string()
    };
    flush_response(res_ptr, Some(text_str.as_bytes()), Some("text/plain; charset=utf-8"), 200);
    Value::null()
}

pub fn native_context_status(args: Vec<Value>) -> Value {
    if !args.is_empty() && args[0].is_number() {
        let code = args[0].as_number() as u16;
        ACTIVE_RESPONSE_STATE.with(|s| s.borrow_mut().status = Some(code));
    }
    Value::null()
}

pub fn native_context_header(args: Vec<Value>) -> Value {
    if args.len() == 1 {
        let name = match args[0].as_str() {
            Some(s) => s.to_ascii_lowercase(),
            None => return Value::null(),
        };
        let val_opt = ACTIVE_REQUEST_HEADERS.with(|h| h.borrow().get(&name).cloned());
        if let Some(val) = val_opt {
            let ptr = get_or_create_string(&val);
            Value::string(ptr)
        } else {
            Value::null()
        }
    } else if args.len() >= 2 {
        let name = match args[0].as_str() {
            Some(s) => s.to_string(),
            None => return Value::null(),
        };
        let val = match args[1].as_str() {
            Some(s) => s.to_string(),
            None => args[1].to_string(),
        };
        ACTIVE_RESPONSE_STATE.with(|s| {
            s.borrow_mut().headers.push((name, val));
        });
        Value::null()
    } else {
        Value::null()
    }
}

pub fn native_req_header(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let name = match args[0].as_str() {
        Some(s) => s.to_ascii_lowercase(),
        None => return Value::null(),
    };
    let val_opt = ACTIVE_REQUEST_HEADERS.with(|h| h.borrow().get(&name).cloned());
    if let Some(val) = val_opt {
        let ptr = get_or_create_string(&val);
        Value::string(ptr)
    } else {
        Value::null()
    }
}

pub fn native_req_cookie(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let name = match args[0].as_str() {
        Some(s) => s,
        None => return Value::null(),
    };
    let val_opt = ACTIVE_REQUEST_COOKIES.with(|c| c.borrow().get(name).cloned());
    if let Some(val) = val_opt {
        let ptr = get_or_create_string(&val);
        Value::string(ptr)
    } else {
        Value::null()
    }
}

pub fn native_req_query(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let name = match args[0].as_str() {
        Some(s) => s,
        None => return Value::null(),
    };
    let val_opt = ACTIVE_REQUEST_QUERY.with(|q| q.borrow().get(name).cloned());
    if let Some(val) = val_opt {
        let ptr = get_or_create_string(&val);
        Value::string(ptr)
    } else {
        Value::null()
    }
}

pub fn native_res_get_header(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let name = match args[0].as_str() {
        Some(s) => s.to_ascii_lowercase(),
        None => return Value::null(),
    };
    let val_opt = ACTIVE_RESPONSE_STATE.with(|s| {
        for (k, v) in &s.borrow().headers {
            if k.eq_ignore_ascii_case(&name) {
                return Some(v.clone());
            }
        }
        None
    });
    if let Some(val) = val_opt {
        let ptr = get_or_create_string(&val);
        Value::string(ptr)
    } else {
        Value::null()
    }
}

pub fn native_context_set_cookie(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::null();
    }
    let name = match args[0].as_str() {
        Some(s) => s,
        None => return Value::null(),
    };
    let val = match args[1].as_str() {
        Some(s) => s.to_string(),
        None => args[1].to_string(),
    };
    let options = if args.len() >= 3 { Some(args[2]) } else { None };
    let cookie_str = format_cookie(name, &val, options);
    ACTIVE_RESPONSE_STATE.with(|s| {
        s.borrow_mut().cookies.push(cookie_str);
    });
    Value::null()
}

pub fn native_context_clear_cookie(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let name = match args[0].as_str() {
        Some(s) => s,
        None => return Value::null(),
    };
    let path = if args.len() >= 2 && args[1].is_object() {
        let ptr = args[1].as_gc_ptr();
        unsafe {
            if let GcData::Object(map) = &(*ptr).data {
                map.get(&crate::vm::value::MapKey(Value::string(get_or_create_string("path"))))
                    .and_then(|v| v.as_str())
                    .unwrap_or("/")
            } else {
                "/"
            }
        }
    } else {
        "/"
    };
    let cookie_str = format!("{}=; Path={}; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:00 GMT", name, path);
    ACTIVE_RESPONSE_STATE.with(|s| {
        s.borrow_mut().cookies.push(cookie_str);
    });
    Value::null()
}

pub fn native_context_redirect(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let res_ptr = ACTIVE_HTTP_RESPONSE.with(|resp| resp.get());
    if res_ptr.is_null() {
        return Value::null();
    }
    let url_str = match args[0].as_str() {
        Some(s) => s.to_string(),
        None => return Value::null(),
    };
    let status_code = if args.len() >= 2 && args[1].is_number() {
        args[1].as_number() as u16
    } else {
        302
    };
    ACTIVE_RESPONSE_STATE.with(|s| {
        s.borrow_mut().headers.push(("Location".to_string(), url_str));
    });
    flush_response(res_ptr, Some(b""), None, status_code);
    Value::null()
}

pub fn native_context_serve_static(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let res_ptr = ACTIVE_HTTP_RESPONSE.with(|resp| resp.get());
    if res_ptr.is_null() {
        return Value::null();
    }
    let path_str = match args[0].as_str() {
        Some(s) => s.to_string(),
        None => return Value::null(),
    };
    let headers = ACTIVE_REQUEST_HEADERS.with(|h| h.borrow().clone());
    let file_path = Path::new(&path_str);
    if !serve_static_file(res_ptr, file_path, &headers) {
        unsafe {
            let status = CString::new("404 Not Found").unwrap();
            er_http_response_write_status(res_ptr, status.as_ptr(), status.as_bytes().len());
            let not_found = "404 Not Found";
            er_http_response_end(res_ptr, not_found.as_ptr() as *const c_char, not_found.len());
        }
    }
    Value::null()
}

pub fn native_res_end(args: Vec<Value>) -> Value {
    let res_ptr = ACTIVE_HTTP_RESPONSE.with(|resp| resp.get());
    if res_ptr.is_null() {
        return Value::null();
    }
    let body_bytes = if !args.is_empty() {
        if let Some(s) = args[0].as_str() {
            s.as_bytes().to_vec()
        } else {
            args[0].to_string().into_bytes()
        }
    } else {
        Vec::new()
    };
    flush_response(res_ptr, Some(&body_bytes), None, 200);
    Value::null()
}
