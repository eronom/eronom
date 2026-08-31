use std::collections::HashMap;
use std::ffi::{c_char, c_void, CString};
use std::fs;
use std::path::Path;
use std::time::SystemTime;
use crate::vm::value::Value;
use super::ffi::*;
use super::types::*;
use super::utils::{calculate_etag, format_http_date, get_mime_type_for_extension, percent_decode};

#[derive(Debug, PartialEq, Eq)]
pub enum RangeHeader {
    Satisfiable(u64, u64),
    Unsatisfiable,
    None,
}

pub fn parse_range_header(range_str: &str, file_len: u64) -> RangeHeader {
    let trimmed = range_str.trim();
    if !trimmed.starts_with("bytes=") {
        return RangeHeader::None;
    }
    let range_spec = &trimmed["bytes=".len()..];
    let first_range = range_spec.split(',').next().unwrap_or("").trim();
    if let Some(dash_pos) = first_range.find('-') {
        let start_str = first_range[..dash_pos].trim();
        let end_str = first_range[dash_pos + 1..].trim();

        if start_str.is_empty() {
            if let Ok(suffix_len) = end_str.parse::<u64>() {
                if suffix_len == 0 {
                    return RangeHeader::Unsatisfiable;
                }
                let start = if suffix_len >= file_len { 0 } else { file_len - suffix_len };
                let end = if file_len > 0 { file_len - 1 } else { 0 };
                return RangeHeader::Satisfiable(start, end);
            }
        } else if end_str.is_empty() {
            if let Ok(start) = start_str.parse::<u64>() {
                if start >= file_len {
                    return RangeHeader::Unsatisfiable;
                }
                let end = if file_len > 0 { file_len - 1 } else { 0 };
                return RangeHeader::Satisfiable(start, end);
            }
        } else if let (Ok(start), Ok(end)) = (start_str.parse::<u64>(), end_str.parse::<u64>()) {
            if start > end || start >= file_len {
                return RangeHeader::Unsatisfiable;
            }
            let end = end.min(if file_len > 0 { file_len - 1 } else { 0 });
            return RangeHeader::Satisfiable(start, end);
        }
    }
    RangeHeader::Unsatisfiable
}

pub fn serve_static_file(
    res_ptr: *mut c_void,
    file_path: &Path,
    req_headers: &HashMap<String, String>,
) -> bool {
    let path_str = file_path.to_string_lossy().to_string();

    if !file_path.exists() || !file_path.is_file() {
        if let Some(vfs_bytes) = crate::vm::embedded::get_vfs_file(&path_str) {
            let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let mime = get_mime_type_for_extension(ext);
            let etag = format!("\"vfs-{}\"", vfs_bytes.len());

            if let Some(if_none_match) = req_headers.get("if-none-match") {
                if if_none_match.contains(&etag) || if_none_match.trim() == "*" {
                    unsafe {
                        let status = CString::new("304 Not Modified").unwrap();
                        er_http_response_write_status(res_ptr, status.as_ptr(), status.as_bytes().len());
                        let etag_key = CString::new("ETag").unwrap();
                        let etag_val = CString::new(etag).unwrap();
                        er_http_response_write_header(res_ptr, etag_key.as_ptr(), etag_key.as_bytes().len(), etag_val.as_ptr(), etag_val.as_bytes().len());
                        er_http_response_end(res_ptr, b"".as_ptr() as *const c_char, 0);
                    }
                    return true;
                }
            }

            unsafe {
                let status = CString::new("200 OK").unwrap();
                er_http_response_write_status(res_ptr, status.as_ptr(), status.as_bytes().len());

                let ct_k = CString::new("Content-Type").unwrap();
                let ct_v = CString::new(mime).unwrap();
                er_http_response_write_header(res_ptr, ct_k.as_ptr(), ct_k.as_bytes().len(), ct_v.as_ptr(), ct_v.as_bytes().len());

                let etag_k = CString::new("ETag").unwrap();
                let etag_v = CString::new(etag).unwrap();
                er_http_response_write_header(res_ptr, etag_k.as_ptr(), etag_k.as_bytes().len(), etag_v.as_ptr(), etag_v.as_bytes().len());

                let cl_k = CString::new("Content-Length").unwrap();
                let cl_v = CString::new(format!("{}", vfs_bytes.len())).unwrap();
                er_http_response_write_header(res_ptr, cl_k.as_ptr(), cl_k.as_bytes().len(), cl_v.as_ptr(), cl_v.as_bytes().len());

                er_http_response_end(res_ptr, vfs_bytes.as_ptr() as *const c_char, vfs_bytes.len());
            }
            return true;
        }
        return false;
    }

    let metadata = match fs::metadata(file_path) {
        Ok(m) => m,
        Err(_) => return false,
    };

    let file_len = metadata.len();
    let mtime = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let etag = calculate_etag(file_len, mtime);
    let last_modified = format_http_date(mtime);

    let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let mime = get_mime_type_for_extension(ext);

    // 1. Conditional GET: If-None-Match
    if let Some(if_none_match) = req_headers.get("if-none-match") {
        if if_none_match.contains(&etag) || if_none_match.trim() == "*" {
            unsafe {
                let status = CString::new("304 Not Modified").unwrap();
                er_http_response_write_status(res_ptr, status.as_ptr(), status.as_bytes().len());
                let etag_key = CString::new("ETag").unwrap();
                let etag_val = CString::new(etag).unwrap();
                er_http_response_write_header(res_ptr, etag_key.as_ptr(), etag_key.as_bytes().len(), etag_val.as_ptr(), etag_val.as_bytes().len());
                er_http_response_end(res_ptr, b"".as_ptr() as *const c_char, 0);
            }
            return true;
        }
    }

    // 2. Conditional GET: If-Modified-Since
    if let Some(if_mod_since) = req_headers.get("if-modified-since") {
        if if_mod_since.trim() == last_modified.as_str() {
            unsafe {
                let status = CString::new("304 Not Modified").unwrap();
                er_http_response_write_status(res_ptr, status.as_ptr(), status.as_bytes().len());
                let etag_key = CString::new("ETag").unwrap();
                let etag_val = CString::new(etag).unwrap();
                er_http_response_write_header(res_ptr, etag_key.as_ptr(), etag_key.as_bytes().len(), etag_val.as_ptr(), etag_val.as_bytes().len());
                er_http_response_end(res_ptr, b"".as_ptr() as *const c_char, 0);
            }
            return true;
        }
    }

    // 3. Range Requests
    if let Some(range_header) = req_headers.get("range") {
        match parse_range_header(range_header, file_len) {
            RangeHeader::Satisfiable(start, end) => {
                let chunk_len = (end - start + 1) as usize;
                let mut buffer = vec![0u8; chunk_len];
                use std::io::{Read, Seek, SeekFrom};
                if let Ok(mut f) = fs::File::open(file_path) {
                    if f.seek(SeekFrom::Start(start)).is_ok() && f.read_exact(&mut buffer).is_ok() {
                        unsafe {
                            let status = CString::new("206 Partial Content").unwrap();
                            er_http_response_write_status(res_ptr, status.as_ptr(), status.as_bytes().len());
                            
                            let ct_k = CString::new("Content-Type").unwrap();
                            let ct_v = CString::new(mime).unwrap();
                            er_http_response_write_header(res_ptr, ct_k.as_ptr(), ct_k.as_bytes().len(), ct_v.as_ptr(), ct_v.as_bytes().len());
                            
                            let cr_k = CString::new("Content-Range").unwrap();
                            let cr_v = CString::new(format!("bytes {}-{}/{}", start, end, file_len)).unwrap();
                            er_http_response_write_header(res_ptr, cr_k.as_ptr(), cr_k.as_bytes().len(), cr_v.as_ptr(), cr_v.as_bytes().len());
                            
                            let cl_k = CString::new("Content-Length").unwrap();
                            let cl_v = CString::new(format!("{}", chunk_len)).unwrap();
                            er_http_response_write_header(res_ptr, cl_k.as_ptr(), cl_k.as_bytes().len(), cl_v.as_ptr(), cl_v.as_bytes().len());

                            let ar_k = CString::new("Accept-Ranges").unwrap();
                            let ar_v = CString::new("bytes").unwrap();
                            er_http_response_write_header(res_ptr, ar_k.as_ptr(), ar_k.as_bytes().len(), ar_v.as_ptr(), ar_v.as_bytes().len());

                            let etag_k = CString::new("ETag").unwrap();
                            let etag_v = CString::new(etag).unwrap();
                            er_http_response_write_header(res_ptr, etag_k.as_ptr(), etag_k.as_bytes().len(), etag_v.as_ptr(), etag_v.as_bytes().len());

                            er_http_response_end(res_ptr, buffer.as_ptr() as *const c_char, buffer.len());
                        }
                        return true;
                    }
                }
            }
            RangeHeader::Unsatisfiable => {
                unsafe {
                    let status = CString::new("416 Range Not Satisfiable").unwrap();
                    er_http_response_write_status(res_ptr, status.as_ptr(), status.as_bytes().len());
                    let cr_k = CString::new("Content-Range").unwrap();
                    let cr_v = CString::new(format!("bytes */{}", file_len)).unwrap();
                    er_http_response_write_header(res_ptr, cr_k.as_ptr(), cr_k.as_bytes().len(), cr_v.as_ptr(), cr_v.as_bytes().len());
                    er_http_response_end(res_ptr, b"".as_ptr() as *const c_char, 0);
                }
                return true;
            }
            RangeHeader::None => {}
        }
    }

    // 4. Full file response
    if let Ok(content) = fs::read(file_path) {
        unsafe {
            let status = CString::new("200 OK").unwrap();
            er_http_response_write_status(res_ptr, status.as_ptr(), status.as_bytes().len());

            let ct_k = CString::new("Content-Type").unwrap();
            let ct_v = CString::new(mime).unwrap();
            er_http_response_write_header(res_ptr, ct_k.as_ptr(), ct_k.as_bytes().len(), ct_v.as_ptr(), ct_v.as_bytes().len());

            let ar_k = CString::new("Accept-Ranges").unwrap();
            let ar_v = CString::new("bytes").unwrap();
            er_http_response_write_header(res_ptr, ar_k.as_ptr(), ar_k.as_bytes().len(), ar_v.as_ptr(), ar_v.as_bytes().len());

            let etag_k = CString::new("ETag").unwrap();
            let etag_v = CString::new(etag).unwrap();
            er_http_response_write_header(res_ptr, etag_k.as_ptr(), etag_k.as_bytes().len(), etag_v.as_ptr(), etag_v.as_bytes().len());

            let lm_k = CString::new("Last-Modified").unwrap();
            let lm_v = CString::new(last_modified).unwrap();
            er_http_response_write_header(res_ptr, lm_k.as_ptr(), lm_k.as_bytes().len(), lm_v.as_ptr(), lm_v.as_bytes().len());

            let cl_k = CString::new("Content-Length").unwrap();
            let cl_v = CString::new(format!("{}", content.len())).unwrap();
            er_http_response_write_header(res_ptr, cl_k.as_ptr(), cl_k.as_bytes().len(), cl_v.as_ptr(), cl_v.as_bytes().len());

            er_http_response_end(res_ptr, content.as_ptr() as *const c_char, content.len());
        }
        return true;
    }

    false
}

pub fn native_static_route_handler(_args: Vec<Value>) -> Value {
    let res_ptr = ACTIVE_HTTP_RESPONSE.with(|resp| resp.get());
    if res_ptr.is_null() {
        return Value::null();
    }
    let active_headers = ACTIVE_REQUEST_HEADERS.with(|h| h.borrow().clone());
    let req_path = ACTIVE_REQUEST_PATH.with(|p| p.borrow().clone());

    let mut served = false;
    STATIC_MOUNTS.with(|mounts| {
        for (prefix, root) in mounts.borrow().iter() {
            if req_path == *prefix || req_path.starts_with(&format!("{}/", prefix)) {
                let rel = if req_path == *prefix {
                    ""
                } else {
                    &req_path[prefix.len() + 1..]
                };
                
                let decoded_rel = percent_decode(rel);
                if decoded_rel.contains("..") {
                    continue;
                }
                
                let base = Path::new(root);
                let mut target_path = if decoded_rel.is_empty() {
                    base.join("index.html")
                } else {
                    base.join(&decoded_rel)
                };

                if target_path.is_dir() {
                    target_path = target_path.join("index.html");
                }

                if target_path.exists() && target_path.is_file() {
                    if serve_static_file(res_ptr, &target_path, &active_headers) {
                        served = true;
                        break;
                    }
                }
            }
        }
    });

    if !served {
        unsafe {
            let status = CString::new("404 Not Found").unwrap();
            er_http_response_write_status(res_ptr, status.as_ptr(), status.as_bytes().len());
            let not_found = "404 Not Found";
            er_http_response_end(res_ptr, not_found.as_ptr() as *const c_char, not_found.len());
        }
    }

    Value::null()
}
