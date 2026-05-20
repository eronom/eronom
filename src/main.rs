use std::path::{Path, PathBuf};
use eronom::er;
use eronom::compiler;
use tiny_http::{Server, Response, Header};
use std::fs;
use std::collections::HashMap;
use serde_json;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mut cmd = "dev";
    let mut dir = ".";
    let mut port: u16 = 8080;

    if args.len() > 1 {
        let first_arg = &args[1];
        if matches!(first_arg.as_str(), "build" | "dev" | "start" | "init") {
            cmd = first_arg;
            if args.len() > 2 {
                dir = &args[2];
            }
            if args.len() > 3 {
                port = args[3].parse().unwrap_or(8080);
            }
        } else if first_arg.ends_with(".er") {
            er::run_file(first_arg)?;
            return Ok(());
        } else {
            dir = first_arg;
        }
    }

    match cmd {
        "init" => init_project(dir)?,
        "build" => build_project(dir)?,
        "start" => start_server(dir, true, port)?,
        "dev" => start_server(dir, false, port)?,
        _ => anyhow::bail!("Unknown command: {}", cmd),
    }

    Ok(())
}

fn init_project(dir: &str) -> anyhow::Result<()> {
    println!("Initializing fresh Eronom project in {}", dir);
    fs::create_dir_all(dir)?;
    Ok(())
}

fn build_project(dir: &str) -> anyhow::Result<()> {
    let build_dir = Path::new(dir).join("build");
    println!("Building project to {:?}", build_dir);
    if build_dir.exists() {
        fs::remove_dir_all(&build_dir)?;
    }
    fs::create_dir_all(&build_dir)?;

    let base_path = fs::canonicalize(dir)?;
    build_dir_recursive(&base_path, &base_path, &build_dir)?;

    Ok(())
}

fn build_dir_recursive(root: &Path, current: &Path, build_root: &Path) -> anyhow::Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str.starts_with('.') || name_str == "target" || name_str == "build" || name_str == "node_modules" || name_str == "src" {
            continue;
        }

        if path.is_dir() {
            build_dir_recursive(root, &path, build_root)?;
        } else {
            let rel_path = path.strip_prefix(root)?;
            let dest_path = build_root.join(rel_path);

            if name_str.ends_with(".erm") {
                if name_str == "layout.erm" {
                    continue;
                }
                // Skip components (starts with uppercase)
                if name_str.chars().next().unwrap().is_ascii_uppercase() {
                    continue;
                }

                let content = fs::read_to_string(&path)?;
                let parent = path.parent().unwrap().to_string_lossy();
                let params = HashMap::new();
                match compiler::process_erm_component(&parent, &content, true, &params) {
                    Ok(processed) => {
                        let mut html_dest = dest_path.clone();
                        if name_str == "page.erm" || name_str == "index.erm" {
                            html_dest.set_file_name("index.html");
                        } else {
                            html_dest.set_extension("html");
                        }
                        if let Some(parent) = html_dest.parent() {
                            fs::create_dir_all(parent)?;
                        }
                        fs::write(html_dest, processed)?;
                    }
                    Err(e) => {
                        eprintln!("Error compiling {}: {}", path.display(), e);
                    }
                }
            }
        }
    }
    Ok(())
}

use std::sync::{Arc, Mutex, Condvar};
use std::thread;
use std::time::{Duration, SystemTime};

struct Watcher {
    last_path: Mutex<Option<String>>,
    change_count: Mutex<usize>,
    cond: Condvar,
}

impl Watcher {
    fn new() -> Self {
        Watcher {
            last_path: Mutex::new(None),
            change_count: Mutex::new(0),
            cond: Condvar::new(),
        }
    }

    fn notify(&self, path: String) {
        let mut count = self.change_count.lock().unwrap();
        *count += 1;
        let mut lp = self.last_path.lock().unwrap();
        *lp = Some(path);
        self.cond.notify_all();
    }

    fn wait(&self, last_count: usize) -> (usize, String) {
        let mut count = self.change_count.lock().unwrap();
        while *count == last_count {
            count = self.cond.wait(count).unwrap();
        }
        let lp = self.last_path.lock().unwrap();
        (*count, lp.clone().unwrap_or_else(|| "unknown".to_string()))
    }
}

fn check_dir_changed(root: &Path, current: &Path, last_check: &mut SystemTime) -> anyhow::Result<Option<String>> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = entry.metadata()?;
        let mtime = metadata.modified()?;

        if mtime > *last_check {
            *last_check = mtime;
            let rel = path.strip_prefix(root).unwrap_or(&path);
            return Ok(Some(rel.to_string_lossy().to_string()));
        }

        if path.is_dir() {
            let name = entry.file_name();
            if name == "target" || name == ".git" || name == "build" || name == "src" { continue; }
            if let Ok(Some(p)) = check_dir_changed(root, &path, last_check) {
                return Ok(Some(p));
            }
        }
    }
    Ok(None)
}

fn watch_files(dir: PathBuf, watcher: Arc<Watcher>) {
    let mut last_check = SystemTime::now();
    loop {
        thread::sleep(Duration::from_millis(200));
        if let Ok(Some(path)) = check_dir_changed(&dir, &dir, &mut last_check) {
            watcher.notify(path);
        }
    }
}

fn start_server(dir: &str, is_prod: bool, port: u16) -> anyhow::Result<()> {
    let server = Arc::new(Server::http(format!("0.0.0.0:{}", port)).map_err(|e| anyhow::anyhow!(e))?);
    println!("{} server running at http://localhost:{}", if is_prod { "Production" } else { "Dev" }, port);

    let base_path = fs::canonicalize(dir)?;
    let watcher = Arc::new(Watcher::new());

    if !is_prod {
        let w = Arc::clone(&watcher);
        let d = base_path.clone();
        thread::spawn(move || {
            watch_files(d, w);
        });
    }

    for mut request in server.incoming_requests() {
        let base_path = base_path.clone();
        let watcher = Arc::clone(&watcher);
        
        thread::spawn(move || {
            let url = request.url().to_string();
            let method = request.method().to_string();
            let mut target = &url[..];
            if let Some(idx) = target.find('?') { target = &target[..idx]; }
            if let Some(idx) = target.find('#') { target = &target[..idx]; }

            println!("Request: {} {}", method, target);

            if !is_prod && target == "/__hmr" {
                let last_count: usize = url.find("v=").and_then(|idx| {
                    url[idx+2..].split('&').next().and_then(|s| s.parse().ok())
                }).unwrap_or(0);
                let (new_count, path) = watcher.wait(last_count);
                let json = serde_json::json!({
                    "type": "update",
                    "path": path,
                    "version": new_count
                });
                let response = Response::from_string(json.to_string())
                    .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap())
                    .with_header(Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap());
                request.respond(response).ok();
                return;
            }

            // API Routing
            let mut api_file = None;
            let mut api_base_path = "/".to_string();

            if target.starts_with("/api") {
                let rel_path = target.trim_start_matches('/');
                
                // 1. Try [path]/route.er
                let route_er = base_path.join(rel_path).join("route.er");
                if route_er.exists() {
                    api_file = Some(route_er);
                    api_base_path = target.to_string();
                } else {
                    // 2. Try [path].er
                    let direct_er = base_path.join(format!("{}.er", rel_path));
                    if direct_er.exists() {
                        api_file = Some(direct_er);
                        api_base_path = target.to_string();
                    }
                }
            }

            if api_file.is_none() {
                let server_er = base_path.join("server.er");
                if server_er.exists() {
                    api_file = Some(server_er);
                    api_base_path = "/".to_string();
                }
            }

            if let Some(file) = api_file {
                match er::handle_api_request(&mut request, file.to_str().unwrap(), &api_base_path) {
                    Ok(Some(response)) => {
                        request.respond(response).ok();
                        return;
                    }
                    Ok(None) => {}
                    Err(e) => {
                        eprintln!("API Error: {}", e);
                        let response = Response::from_string(format!("API Error: {}", e)).with_status_code(500);
                        request.respond(response).ok();
                        return;
                    }
                }
            }

            if target.ends_with(".erm") {
                let response = Response::from_string("Not Found").with_status_code(404);
                request.respond(response).ok();
                return;
            }

            let (file_path, params) = if let Some(res) = resolve_dynamic_route(&base_path, target) {
                res
            } else {
                (base_path.join(&target[1..]), HashMap::new())
            };

            if file_path.exists() && file_path.is_file() {
                if file_path.extension().map_or(false, |ext| ext == "erm") {
                    let content = fs::read_to_string(&file_path).unwrap_or_default();
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
                    let content = fs::read(&file_path).unwrap_or_default();
                    let response = Response::from_data(content);
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

fn resolve_dynamic_route(root: &Path, target: &str) -> Option<(PathBuf, HashMap<String, String>)> {
    let target = target.split('?').next().unwrap_or(target);
    let target = target.split('#').next().unwrap_or(target);
    let target = target.trim_start_matches('/');
    
    let segments: Vec<&str> = if target.is_empty() {
        vec![]
    } else {
        target.split('/').collect()
    };

    find_recursive(root, &segments, 0)
}

fn find_recursive(current_dir: &Path, segments: &[&str], index: usize) -> Option<(PathBuf, HashMap<String, String>)> {
    if index == segments.len() {
        let index_erm = current_dir.join("index.erm");
        if index_erm.exists() { return Some((index_erm, HashMap::new())); }
        let page_erm = current_dir.join("page.erm");
        if page_erm.exists() { return Some((page_erm, HashMap::new())); }
        return None;
    }

    let segment = segments[index];

    // 1. Try exact match directory
    let dir_path = current_dir.join(segment);
    if dir_path.is_dir() {
        if let Some(res) = find_recursive(&dir_path, segments, index + 1) {
            return Some(res);
        }
    }

    // 2. Try exact match file (for last segment)
    if index == segments.len() - 1 {
        let erm_path = current_dir.join(format!("{}.erm", segment));
        if erm_path.is_file() {
            return Some((erm_path, HashMap::new()));
        }
        let direct_path = current_dir.join(segment);
        if direct_path.is_file() {
            return Some((direct_path, HashMap::new()));
        }
    }

    // 3. Try dynamic segments
    if let Ok(entries) = fs::read_dir(current_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            
            if name_str.starts_with('[') {
                if name_str.ends_with(']') {
                    // It's a directory [param]
                    let param_name = &name_str[1..name_str.len() - 1];
                    let path = entry.path();
                    if path.is_dir() {
                        if let Some((f_path, mut params)) = find_recursive(&path, segments, index + 1) {
                            params.insert(param_name.to_string(), segment.to_string());
                            return Some((f_path, params));
                        }
                    }
                } else if name_str.ends_with("].erm") && index == segments.len() - 1 {
                    // It's a file [param].erm
                    let param_name = &name_str[1..name_str.len() - 5];
                    let mut params = HashMap::new();
                    params.insert(param_name.to_string(), segment.to_string());
                    return Some((entry.path(), params));
                }
            }
        }
    }

    None
}
