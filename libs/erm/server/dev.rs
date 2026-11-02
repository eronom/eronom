use std::collections::HashMap;
use std::ffi::CString;
use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, SystemTime};
use super::types::*;
use super::ffi::*;
use super::router::scan_directory;
use super::handler::{dev_http_callback, dev_ws_open_callback, dev_ws_message_callback, dev_ws_close_callback, check_hmr_queue};

pub fn start_server(dir: &str, is_prod: bool, port: u16) -> anyhow::Result<()> {
    let mut base_path = fs::canonicalize(dir)?;
    let mut default_file = None;

    if base_path.is_file() {
        default_file = Some(base_path.clone());
        if let Some(parent) = base_path.parent() {
            base_path = parent.to_path_buf();
        }
    } else {
        let toml_path = base_path.join("eronom.toml");
        if toml_path.exists() {
            if let Ok(content) = fs::read_to_string(&toml_path) {
                if let Ok(toml_val) = toml::from_str::<toml::Value>(&content) {
                    if let Some(server) = toml_val.get("server") {
                        if let Some(dev) = server.get("dev") {
                            if let Some(dev_str) = dev.as_str() {
                                let dev_file = base_path.join(dev_str);
                                if dev_file.exists() {
                                    default_file = Some(dev_file);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    *BASE_PATH.lock().unwrap() = Some(base_path.clone());
    *DEFAULT_FILE.lock().unwrap() = default_file;
    *IS_PROD.lock().unwrap() = is_prod;

    if is_prod {
        preload_static_assets(&base_path);
    }

    crate::vm::er_http::context::set_render_hooks(
        super::render::native_render,
        super::render::native_render_string,
    );

    unsafe {
        er_http_init_with_callbacks(
            dev_http_callback,
            dev_ws_open_callback,
            dev_ws_message_callback,
            dev_ws_close_callback,
        );
    }

    if !is_prod {
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


                    let is_css = rel_path.ends_with(".css") || rel_path == "eronom.toml";
                    let update_type = if is_css { "css-update" } else { "js-update" };
                    let timestamp = SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;

                    let path_str = if rel_path.starts_with('/') {
                        rel_path
                    } else {
                        format!("/{}", rel_path)
                    };

                    let msg = serde_json::json!({
                        "type": "update",
                        "path": path_str,
                        "updates": [{
                            "type": update_type,
                            "path": path_str,
                            "acceptedPath": path_str,
                            "timestamp": timestamp
                        }]
                    }).to_string();
                    
                    HMR_QUEUE.lock().unwrap().push(msg);
                    last_ping = SystemTime::now();
                } else {
                    last_files = current_files;
                    if let Ok(elapsed) = last_ping.elapsed() {
                        if elapsed >= Duration::from_secs(15) {
                            let ping = serde_json::json!({
                                "type": "ping"
                            }).to_string();
                            HMR_QUEUE.lock().unwrap().push(ping);
                            last_ping = SystemTime::now();
                        }
                    }
                }
            }
        });

        unsafe {
            let hmr_route = CString::new("/__hmr").unwrap();
            er_ws_register_route(hmr_route.as_ptr());
            er_http_create_timer(200, check_hmr_queue);
        }
    }

    let initial_port = port;
    let port = find_available_port(initial_port);
    if port != initial_port {
        println!("Port {} is already in use, trying port {} instead.", initial_port, port);
    }

    println!("{} server running at http://localhost:{}", if is_prod { "Production" } else { "Dev" }, port);

    unsafe {
        er_http_listen_and_run(port as i32);
    }

    Ok(())
}

pub use crate::vm::er_http::utils::{is_port_in_use, find_available_port};

pub fn preload_static_assets(root: &std::path::Path) {
    let mut cache = STATIC_ASSET_CACHE.lock().unwrap();
    let mut manifest = ASSET_MANIFEST.lock().unwrap();
    cache.clear();
    manifest.clear();

    // Check for manifest.json in root or root/build
    let manifest_path = if root.join("manifest.json").exists() {
        Some(root.join("manifest.json"))
    } else if root.join("build/manifest.json").exists() {
        Some(root.join("build/manifest.json"))
    } else {
        None
    };

    if let Some(m_path) = manifest_path {
        if let Ok(content) = fs::read_to_string(&m_path) {
            if let Ok(parsed) = serde_json::from_str::<HashMap<String, String>>(&content) {
                *manifest = parsed;
            }
        }
    }

    // Preload runtime.js from disk or embedded template
    let runtime_content = if root.join("modules/erm/runtime.js").exists() {
        fs::read_to_string(root.join("modules/erm/runtime.js")).unwrap_or_default()
    } else {
        include_str!("../../init/modules/erm/runtime.js").to_string()
    };
    let prod_runtime = runtime_content.replace(
        "import { createHotContext, showError } from './hmr.js';",
        "const createHotContext = () => ({ data: {} }); const showError = (e) => { console.error(e); };",
    );
    let minified_runtime = crate::compiler::minify_js(&prod_runtime);
    let runtime_hash = crate::compiler::compute_asset_hash(minified_runtime.as_bytes());
    let runtime_etag = format!("\"{}\"", runtime_hash);

    let runtime_cached = CachedAsset {
        content: std::sync::Arc::new(minified_runtime.into_bytes()),
        content_type: "application/javascript; charset=utf-8",
        etag: runtime_etag.clone(),
        is_immutable: false,
    };
    cache.insert("/modules/erm/runtime.js".to_string(), runtime_cached.clone());
    cache.insert(format!("/assets/runtime-{}.js", runtime_hash), CachedAsset {
        content: runtime_cached.content.clone(),
        content_type: runtime_cached.content_type,
        etag: runtime_etag,
        is_immutable: true,
    });

    fn walk_assets(base: &std::path::Path, dir: &std::path::Path, cache: &mut HashMap<String, CachedAsset>) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with('.') || name_str == "target" || name_str == "deps" || name_str == "hmr.js" {
                    continue;
                }
                if path.is_dir() {
                    walk_assets(base, &path, cache);
                } else if path.is_file() {
                    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                    if ext == "erm" || ext == "er" {
                        continue;
                    }
                    if let Ok(rel) = path.strip_prefix(base) {
                        let rel_str = rel.to_string_lossy().replace('\\', "/");
                        let route_key = if rel_str.starts_with('/') {
                            rel_str
                        } else {
                            format!("/{}", rel_str)
                        };
                        if let Ok(data) = fs::read(&path) {
                            let hash = crate::compiler::compute_asset_hash(&data);
                            let is_immutable = route_key.starts_with("/assets/");
                            let mime = guess_mime_type(ext);
                            cache.insert(route_key, CachedAsset {
                                content: std::sync::Arc::new(data),
                                content_type: mime,
                                etag: format!("\"{}\"", hash),
                                is_immutable,
                            });
                        }
                    }
                }
            }
        }
    }

    walk_assets(root, root, &mut cache);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn test_find_available_port_increments_when_busy() {
        let initial_listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = initial_listener.local_addr().unwrap().port();

        assert!(is_port_in_use(port));

        let next_port = find_available_port(port);
        assert!(next_port > port);
        assert!(!is_port_in_use(next_port));

        drop(initial_listener);
    }
}


