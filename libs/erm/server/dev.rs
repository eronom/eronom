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

    // Compile global ermcss styles on start if enabled
    let ermcss_cfg = crate::compiler::parse_ermcss_config(&base_path);
    let mut ermcss_enabled = ermcss_cfg.enabled;
    let mut ermcss_globs = ermcss_cfg.content;

    if ermcss_enabled {
        match crate::compiler::compile_project_ermcss(&base_path, &ermcss_globs) {
            Ok(css) => {
                crate::compiler::set_global_ermcss(css);
            }
            Err(e) => {
                eprintln!("[Warning] Failed to compile global ermcss styles: {}", e);
            }
        }
    } else {
        crate::compiler::set_global_ermcss(String::new());
    }

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

                    if ermcss_enabled {
                        let should_recompile = rel_path == "eronom.toml" || ermcss_globs.iter().any(|glob| {
                            crate::compiler::matches_glob(&watch_path, &path, glob)
                        });
                        
                        if should_recompile {
                            if rel_path == "eronom.toml" {
                                let new_cfg = crate::compiler::parse_ermcss_config(&watch_path);
                                ermcss_enabled = new_cfg.enabled;
                                ermcss_globs = new_cfg.content;
                            }
                            
                            if ermcss_enabled {
                                match crate::compiler::compile_project_ermcss(&watch_path, &ermcss_globs) {
                                    Ok(css) => {
                                        crate::compiler::set_global_ermcss(css);
                                    }
                                    Err(e) => {
                                        eprintln!("[Warning] Failed to recompile global ermcss styles: {}", e);
                                    }
                                }
                            } else {
                                crate::compiler::set_global_ermcss(String::new());
                            }
                        }
                    }
                    
                    let path_str = if rel_path.starts_with('/') {
                        rel_path
                    } else {
                        format!("/{}", rel_path)
                    };
                    
                    let msg = serde_json::json!({
                        "type": "update",
                        "path": path_str
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

    println!("{} server running at http://localhost:{}", if is_prod { "Production" } else { "Dev" }, port);

    unsafe {
        er_http_listen_and_run(port as i32);
    }

    Ok(())
}
