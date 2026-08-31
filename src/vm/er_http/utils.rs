use std::collections::HashMap;
use std::time::SystemTime;
use crate::vm::value::Value;
use crate::vm::gc::GcData;

pub fn percent_decode(input: &str) -> String {
    let mut bytes = Vec::with_capacity(input.len());
    let input_bytes = input.as_bytes();
    let mut i = 0;
    while i < input_bytes.len() {
        if input_bytes[i] == b'%' && i + 2 < input_bytes.len() {
            if let Ok(byte) = u8::from_str_radix(std::str::from_utf8(&input_bytes[i+1..i+2+1]).unwrap_or(""), 16) {
                bytes.push(byte);
                i += 3;
                continue;
            }
        } else if input_bytes[i] == b'+' {
            bytes.push(b' ');
            i += 1;
            continue;
        }
        bytes.push(input_bytes[i]);
        i += 1;
    }
    String::from_utf8(bytes).unwrap_or_else(|_| input.to_string())
}

pub fn parse_query_string(raw: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if raw.is_empty() {
        return map;
    }
    for pair in raw.split('&') {
        if pair.is_empty() {
            continue;
        }
        let mut parts = pair.splitn(2, '=');
        let key = parts.next().unwrap_or("");
        let val = parts.next().unwrap_or("");
        let decoded_key = percent_decode(key);
        let decoded_val = percent_decode(val);
        map.insert(decoded_key, decoded_val);
    }
    map
}

pub fn parse_cookies(cookie_header: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if cookie_header.is_empty() {
        return map;
    }
    for pair in cookie_header.split(';') {
        let trimmed = pair.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut parts = trimmed.splitn(2, '=');
        let key = parts.next().unwrap_or("").trim();
        let val = parts.next().unwrap_or("").trim();
        if !key.is_empty() {
            map.insert(key.to_string(), val.to_string());
        }
    }
    map
}

pub fn format_cookie(name: &str, val: &str, options: Option<Value>) -> String {
    let mut cookie_str = format!("{}={}", name, val);
    if let Some(opt) = options {
        if opt.is_object() {
            let ptr = opt.as_gc_ptr();
            unsafe {
                match &(*ptr).data {
                    GcData::Object(map) => {
                        for (k, v) in map {
                            let k_str = match k.0.as_str() {
                                Some(s) => s.to_lowercase(),
                                None => continue,
                            };
                            match k_str.as_str() {
                                "domain" => {
                                    if let Some(d) = v.as_str() {
                                        cookie_str.push_str(&format!("; Domain={}", d));
                                    }
                                }
                                "path" => {
                                    if let Some(p) = v.as_str() {
                                        cookie_str.push_str(&format!("; Path={}", p));
                                    }
                                }
                                "expires" => {
                                    if let Some(e) = v.as_str() {
                                        cookie_str.push_str(&format!("; Expires={}", e));
                                    }
                                }
                                "maxage" | "max_age" => {
                                    if v.is_number() {
                                        cookie_str.push_str(&format!("; Max-Age={}", v.as_number() as i64));
                                    }
                                }
                                "secure" => {
                                    if v.is_boolean() && v.as_boolean() {
                                        cookie_str.push_str("; Secure");
                                    }
                                }
                                "httponly" | "http_only" => {
                                    if v.is_boolean() && v.as_boolean() {
                                        cookie_str.push_str("; HttpOnly");
                                    }
                                }
                                "samesite" | "same_site" => {
                                    if let Some(s) = v.as_str() {
                                        cookie_str.push_str(&format!("; SameSite={}", s));
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    cookie_str
}

pub fn get_mime_type_for_extension(ext: &str) -> &'static str {
    match ext {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "ogg" => "video/ogg",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "txt" => "text/plain; charset=utf-8",
        "xml" => "application/xml; charset=utf-8",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

pub fn calculate_etag(size: u64, mtime: SystemTime) -> String {
    let dur = mtime.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    let secs = dur.as_secs();
    let nanos = dur.subsec_nanos();
    format!("\"{:x}-{:x}{:x}\"", size, secs, nanos)
}

pub fn format_http_date(time: SystemTime) -> String {
    let dur = time.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    let mut secs = dur.as_secs();
    
    let days = (secs / 86400) as i64;
    secs %= 86400;
    let hours = secs / 3600;
    secs %= 3600;
    let minutes = secs / 60;
    let seconds = secs % 60;
    
    let day_of_week = match (days + 4) % 7 {
        0 => "Sun",
        1 => "Mon",
        2 => "Tue",
        3 => "Wed",
        4 => "Thu",
        5 => "Fri",
        _ => "Sat",
    };
    
    let mut a = days + 2472632;
    let mut b = (4 * a + 3) / 146097;
    let mut c = a - (146097 * b) / 4;
    let mut d = (4 * c + 3) / 1461;
    let mut e = c - (1461 * d) / 4;
    let mut m = (5 * e + 2) / 153;
    let day = e - (153 * m + 2) / 5 + 1;
    let month_num = m + 3 - 12 * (m / 10);
    let year = 100 * b + d - 4800 + (m / 10);
    
    let month = match month_num {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        _ => "Dec",
    };
    
    format!("{}, {:02} {} {} {:02}:{:02}:{:02} GMT", day_of_week, day, month, year, hours, minutes, seconds)
}

pub fn status_code_to_status_line(code: u16) -> &'static str {
    match code {
        200 => "200 OK",
        201 => "201 Created",
        202 => "202 Accepted",
        204 => "204 No Content",
        206 => "206 Partial Content",
        301 => "301 Moved Permanently",
        302 => "302 Found",
        304 => "304 Not Modified",
        307 => "307 Temporary Redirect",
        308 => "308 Permanent Redirect",
        400 => "400 Bad Request",
        401 => "401 Unauthorized",
        403 => "403 Forbidden",
        404 => "404 Not Found",
        405 => "405 Method Not Allowed",
        409 => "409 Conflict",
        416 => "416 Range Not Satisfiable",
        429 => "429 Too Many Requests",
        500 => "500 Internal Server Error",
        501 => "501 Not Implemented",
        502 => "502 Bad Gateway",
        503 => "503 Service Unavailable",
        _ => "200 OK",
    }
}

pub fn value_to_json(val: Value) -> serde_json::Value {
    if val.is_null() {
        serde_json::Value::Null
    } else if val.is_boolean() {
        serde_json::Value::Bool(val.as_boolean())
    } else if val.is_number() {
        let num = val.as_number();
        if let Some(n) = serde_json::Number::from_f64(num) {
            serde_json::Value::Number(n)
        } else {
            serde_json::Value::Null
        }
    } else if val.is_string() {
        val.as_str().map(|s| serde_json::Value::String(s.to_string())).unwrap_or(serde_json::Value::Null)
    } else if val.is_array() {
        unsafe {
            match &(*val.as_gc_ptr()).data {
                GcData::Array(arr) => {
                    let items: Vec<serde_json::Value> = arr.iter().map(|&v| value_to_json(v)).collect();
                    serde_json::Value::Array(items)
                }
                _ => serde_json::Value::Null
            }
        }
    } else if val.is_object() {
        unsafe {
            match &(*val.as_gc_ptr()).data {
                GcData::Object(map) => {
                    let mut obj = serde_json::Map::new();
                    for (k, v) in map {
                        let key_str = match k.0.as_str() {
                            Some(s) => s.to_string(),
                            None => continue,
                        };
                        obj.insert(key_str, value_to_json(*v));
                    }
                    serde_json::Value::Object(obj)
                }
                GcData::Struct(s) => {
                    let mut obj = serde_json::Map::new();
                    for (map_key, &idx) in &s.descriptor.field_indices {
                        let name = map_key.0.as_str().unwrap_or("");
                        obj.insert(name.to_string(), value_to_json(s.fields[idx]));
                    }
                    serde_json::Value::Object(obj)
                }
                _ => serde_json::Value::Null
            }
        }
    } else {
        serde_json::Value::Null
    }
}
