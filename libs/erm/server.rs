use std::path::{Path, PathBuf};
use std::fs;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, Sender};
use std::thread;
use std::time::{Duration, SystemTime};
use std::collections::HashMap;
use tiny_http::{Server, Response, Header};
use crate::compiler;
use base64::{Engine as _, engine::general_purpose};
use sha1::{Sha1, Digest};

fn scan_directory(dir: &Path, files: &mut HashMap<PathBuf, SystemTime>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            if name_str.starts_with('.') ||
               name_str == "target" ||
               name_str == "build" ||
               name_str == "node_modules" {
                continue;
            }

            if path.is_dir() {
                scan_directory(&path, files);
            } else if path.is_file() {
                if let Some(ext) = path.extension() {
                    let ext_str = ext.to_string_lossy();
                    if ext_str == "erm" || ext_str == "css" || ext_str == "js" {
                        if let Ok(metadata) = entry.metadata() {
                            if let Ok(modified) = metadata.modified() {
                                files.insert(path, modified);
                            }
                        }
                    }
                }
            }
        }
    }
}

fn broadcast_hmr(clients: &Mutex<Vec<Sender<String>>>, msg: String) {
    let mut guard = clients.lock().unwrap();
    guard.retain(|tx| {
        tx.send(msg.clone()).is_ok()
    });
}

fn write_ws_text_frame<W: std::io::Write>(writer: &mut W, text: &str) -> std::io::Result<()> {
    let bytes = text.as_bytes();
    let len = bytes.len();
    writer.write_all(&[0x81])?;
    if len < 126 {
        writer.write_all(&[len as u8])?;
    } else if len <= 65535 {
        writer.write_all(&[126])?;
        writer.write_all(&(len as u16).to_be_bytes())?;
    } else {
        writer.write_all(&[127])?;
        writer.write_all(&(len as u64).to_be_bytes())?;
    }
    writer.write_all(bytes)?;
    writer.flush()?;
    Ok(())
}

pub fn start_server(dir: &str, is_prod: bool, port: u16) -> anyhow::Result<()> {
    let server = Server::http(format!("0.0.0.0:{}", port)).map_err(|e| anyhow::anyhow!(e))?;
    println!("{} server running at http://localhost:{}", if is_prod { "Production" } else { "Dev" }, port);

    let mut base_path = fs::canonicalize(dir)?;
    let mut default_file = None;

    if base_path.is_file() {
        default_file = Some(base_path.clone());
        if let Some(parent) = base_path.parent() {
            base_path = parent.to_path_buf();
        }
    }

    let hmr_clients = Arc::new(Mutex::new(Vec::new()));

    if !is_prod {
        let clients_clone = Arc::clone(&hmr_clients);
        let watch_path = base_path.clone();
        thread::spawn(move || {
            let mut last_files: HashMap<PathBuf, SystemTime> = HashMap::new();
            scan_directory(&watch_path, &mut last_files);
            let mut last_ping = SystemTime::now();

            loop {
                thread::sleep(Duration::from_millis(200));
                let mut current_files = HashMap::new();
                scan_directory(&watch_path, &mut current_files);

                let mut changed_file = None;

                for (path, mod_time) in &current_files {
                    match last_files.get(path) {
                        Some(last_time) => {
                            if mod_time > last_time {
                                changed_file = Some(path.clone());
                                break;
                            }
                        }
                        None => {
                            changed_file = Some(path.clone());
                            break;
                        }
                    }
                }

                if changed_file.is_none() {
                    for path in last_files.keys() {
                        if !current_files.contains_key(path) {
                            changed_file = Some(path.clone());
                            break;
                        }
                    }
                }

                if let Some(path) = changed_file {
                    last_files = current_files;
                    let rel_path = path.strip_prefix(&watch_path)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .into_owned();
                    
                    println!("[HMR] File changed: {}", rel_path);
                    
                    let path_str = if rel_path.starts_with('/') {
                        rel_path
                    } else {
                        format!("/{}", rel_path)
                    };
                    
                    let msg = serde_json::json!({
                        "type": "update",
                        "path": path_str
                    }).to_string();
                    
                    broadcast_hmr(&clients_clone, msg);
                    last_ping = SystemTime::now();
                } else {
                    last_files = current_files;
                    if let Ok(elapsed) = last_ping.elapsed() {
                        if elapsed >= Duration::from_secs(15) {
                            let ping = serde_json::json!({
                                "type": "ping"
                            }).to_string();
                            broadcast_hmr(&clients_clone, ping);
                            last_ping = SystemTime::now();
                        }
                    }
                }
            }
        });
    }

    for request in server.incoming_requests() {
        let base_path = base_path.clone();
        let default_file = default_file.clone();
        let hmr_clients = Arc::clone(&hmr_clients);

        thread::spawn(move || {
            let url = request.url();
            let mut target = url;
            if let Some(idx) = target.find('?') { target = &target[..idx]; }
            if let Some(idx) = target.find('#') { target = &target[..idx]; }

            println!("Request: {} {}", request.method(), target);

            if target == "/__hmr" {
                let mut ws_key = None;
                for h in request.headers() {
                    let field_str: &str = h.field.as_str().as_ref();
                    if field_str.eq_ignore_ascii_case("sec-websocket-key") {
                        let value_str: &str = h.value.as_str().as_ref();
                        ws_key = Some(value_str.to_string());
                        break;
                    }
                }

                let Some(key) = ws_key else {
                    let response = Response::from_string("Bad Request: Missing Sec-WebSocket-Key").with_status_code(400);
                    request.respond(response).ok();
                    return;
                };

                let mut hasher = Sha1::new();
                hasher.update(key.as_bytes());
                hasher.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
                let hash = hasher.finalize();
                let accept = general_purpose::STANDARD.encode(&hash);

                let response_headers = format!(
                    "HTTP/1.1 101 Switching Protocols\r\n\
                     Upgrade: websocket\r\n\
                     Connection: Upgrade\r\n\
                     Sec-WebSocket-Accept: {}\r\n\r\n",
                    accept
                );

                let mut writer = request.into_writer();
                if writer.write_all(response_headers.as_bytes()).is_err() {
                    return;
                }
                if writer.flush().is_err() {
                    return;
                }

                let (tx, rx) = channel();
                hmr_clients.lock().unwrap().push(tx);

                while let Ok(msg) = rx.recv() {
                    if write_ws_text_frame(&mut writer, &msg).is_err() {
                        break;
                    }
                }
                return;
            }

            let mut params = HashMap::new();
            let file_path = if target == "/" {
                if let Some(ref def_file) = default_file {
                    def_file.clone()
                } else {
                    let index_erm = base_path.join("index.erm");
                    if index_erm.exists() { index_erm } else { base_path.join("index.html") }
                }
            } else {
                if let Some((path, p)) = resolve_path(&base_path, target) {
                    params = p;
                    path
                } else {
                    base_path.join(&target[1..])
                }
            };

            if file_path.exists() && file_path.is_file() {
                if file_path.extension().map_or(false, |ext| ext == "erm") {
                    let content = match fs::read_to_string(&file_path) {
                        Ok(c) => c,
                        Err(e) => {
                            let response = Response::from_string(format!("Error reading file: {}", e)).with_status_code(500);
                            request.respond(response).ok();
                            return;
                        }
                    };
                    let parent = file_path.parent().unwrap().to_string_lossy();
                    match compiler::process_erm_component(&parent, &content, is_prod, &params) {
                        Ok(processed) => {
                            let response = Response::from_string(processed)
                                .with_header(Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap());
                            request.respond(response).ok();
                        }
                        Err(e) => {
                            let response = Response::from_string(format!("Error: {}", e)).with_status_code(500);
                            request.respond(response).ok();
                        }
                    }
                } else {
                    let content = match fs::read(&file_path) {
                        Ok(c) => c,
                        Err(e) => {
                            let response = Response::from_string(format!("Error reading file: {}", e)).with_status_code(500);
                            request.respond(response).ok();
                            return;
                        }
                    };
                    let mut response = Response::from_data(content);
                    if let Some(ext) = file_path.extension() {
                        let ext_str = ext.to_string_lossy().to_lowercase();
                        let mime = match ext_str.as_str() {
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
                        };
                        if let Some(mime_type) = mime {
                            response = response.with_header(
                                Header::from_bytes(&b"Content-Type"[..], mime_type.as_bytes()).unwrap()
                            );
                        }
                    }
                    request.respond(response).ok();
                }
            } else {
                let response = Response::from_string("Not Found").with_status_code(404);
                request.respond(response).ok();
            }
        });
    }
    Ok(())
}

fn resolve_path(base_path: &Path, target: &str) -> Option<(PathBuf, HashMap<String, String>)> {
    let parts: Vec<&str> = target.split('/').filter(|s| !s.is_empty()).collect();
    let mut current_path = base_path.to_path_buf();
    let mut params = HashMap::new();

    for (i, part) in parts.iter().enumerate() {
        let mut found = false;
        
        let exact = current_path.join(part);
        if exact.exists() {
            current_path = exact;
            found = true;
        } else {
            let erm = current_path.join(format!("{}.erm", part));
            if erm.exists() {
                current_path = erm;
                found = true;
            } else {
                if let Ok(entries) = fs::read_dir(&current_path) {
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().into_owned();
                        if name.starts_with('[') {
                            if name.ends_with(']') {
                                let param_name = &name[1..name.len() - 1];
                                params.insert(param_name.to_string(), part.to_string());
                                current_path.push(name);
                                found = true;
                                break;
                            } else if name.ends_with("].erm") {
                                let param_name = &name[1..name.len() - 5];
                                params.insert(param_name.to_string(), part.to_string());
                                current_path.push(name);
                                found = true;
                                break;
                            }
                        }
                    }
                }
            }
        }
        
        if !found { return None; }
        
        if current_path.is_file() && i < parts.len() - 1 {
            return None;
        }
    }

    if current_path.is_dir() {
        let index_erm = current_path.join("index.erm");
        if index_erm.exists() { return Some((index_erm, params)); }
        let page_erm = current_path.join("page.erm");
        if page_erm.exists() { return Some((page_erm, params)); }
        let index_html = current_path.join("index.html");
        if index_html.exists() { return Some((index_html, params)); }
        let page_html = current_path.join("page.html");
        if page_html.exists() { return Some((page_html, params)); }
    }

    Some((current_path, params))
}
