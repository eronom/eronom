use std::path::Path;
use eronom::compiler;
use tiny_http::{Server, Response, Header};
use std::fs;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mut cmd = "dev";
    let mut dir = ".";
    let mut port: u16 = 8080;
    let mut is_ssr = false;

    if args.len() > 1 {
        let first_arg = &args[1];
        if matches!(first_arg.as_str(), "build" | "dev" | "start" | "init") {
            cmd = first_arg;
            
            let mut pos_args = Vec::new();
            for arg in args.iter().skip(2) {
                if arg == "--ssr" || arg == "-ssr" {
                    is_ssr = true;
                } else if arg.starts_with("--port=") {
                    port = arg[7..].parse().unwrap_or(8080);
                } else if arg.starts_with('-') {
                    // ignore unknown flags for now
                } else {
                    pos_args.push(arg);
                }
            }
            
            if !pos_args.is_empty() {
                dir = pos_args[0];
                if pos_args.len() > 1 && cmd != "build" {
                    port = pos_args[1].parse().unwrap_or(8080);
                }
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
        "build" => build_project(dir, is_ssr)?,
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

fn build_project(dir: &str, is_ssr: bool) -> anyhow::Result<()> {
    let build_dir = Path::new(dir).join("build");
    if is_ssr {
        println!("Building project for SSR to {:?}", build_dir);
    } else {
        println!("Building project (SSG) to {:?}", build_dir);
    }
    
    if build_dir.exists() {
        fs::remove_dir_all(&build_dir)?;
    }
    fs::create_dir_all(&build_dir)?;

    let base_path = fs::canonicalize(dir)?;
    build_dir_recursive(&base_path, &base_path, &build_dir, is_ssr)?;

    if is_ssr {
        // Copy current executable to build folder for deployment
        if let Ok(exe_path) = std::env::current_exe() {
            let exe_name = exe_path.file_name().unwrap();
            let dest_exe = build_dir.join(exe_name);
            println!("Copying binary to {:?}", dest_exe);
            fs::copy(exe_path, dest_exe).ok();
        }
    }

    Ok(())
}

fn build_dir_recursive(root: &Path, current: &Path, build_root: &Path, is_ssr: bool) -> anyhow::Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str.starts_with('.') || 
           name_str == "target" || 
           name_str == "build" || 
           name_str == "node_modules" || 
           name_str == "src" ||
           name_str == "Cargo.toml" ||
           name_str == "Cargo.lock" ||
           name_str == "eronom" ||
           name_str == "LICENSE" ||
           name_str == "README.md"
        {
            continue;
        }

        if path.is_dir() {
            build_dir_recursive(root, &path, build_root, is_ssr)?;
        } else {
            let rel_path = path.strip_prefix(root)?;
            let dest_path = build_root.join(rel_path);
            
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent)?;
            }

            if name_str.ends_with(".erm") {
                if is_ssr {
                    // SSR mode: just copy the .erm file
                    fs::copy(&path, &dest_path)?;
                } else {
                    // SSG mode: compile to .html
                    if name_str == "layout.erm" {
                        continue;
                    }
                    // Skip components (starts with uppercase)
                    if name_str.chars().next().unwrap().is_ascii_uppercase() {
                        continue;
                    }

                    let content = fs::read_to_string(&path)?;
                    let parent_dir = path.parent().unwrap().to_string_lossy();
                    match compiler::process_erm_component(&parent_dir, &content, true, &std::collections::HashMap::new()) {
                        Ok(processed) => {
                            let mut html_dest = dest_path.clone();
                            if name_str == "page.erm" || name_str == "index.erm" {
                                html_dest.set_file_name("index.html");
                            } else {
                                html_dest.set_extension("html");
                            }
                            fs::write(html_dest, processed)?;
                        }
                        Err(e) => {
                            eprintln!("Error compiling {}: {}", path.display(), e);
                        }
                    }
                }
            } else {
                // Copy assets
                fs::copy(&path, &dest_path)?;
            }
        }
    }
    Ok(())
}

fn start_server(dir: &str, is_prod: bool, port: u16) -> anyhow::Result<()> {
    let server = Server::http(format!("0.0.0.0:{}", port)).map_err(|e| anyhow::anyhow!(e))?;
    println!("{} server running at http://localhost:{}", if is_prod { "Production" } else { "Dev" }, port);

    let base_path = fs::canonicalize(dir)?;

    for request in server.incoming_requests() {
        let url = request.url();
        let mut target = url;
        if let Some(idx) = target.find('?') { target = &target[..idx]; }
        if let Some(idx) = target.find('#') { target = &target[..idx]; }

        println!("Request: {} {}", request.method(), target);

        if target.ends_with(".erm") {
            let response = Response::from_string("Not Found").with_status_code(404);
            request.respond(response).ok();
            continue;
        }

        let mut params = std::collections::HashMap::new();
        let file_path = if target == "/" {
            let index_erm = base_path.join("index.erm");
            if index_erm.exists() { index_erm } else { base_path.join("index.html") }
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
                let content = fs::read_to_string(&file_path)?;
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
                let content = fs::read(&file_path)?;
                let response = Response::from_data(content);
                request.respond(response).ok();
            }
        } else {
            let response = Response::from_string("Not Found").with_status_code(404);
            request.respond(response).ok();
        }
    }
    Ok(())
}

fn resolve_path(base_path: &Path, target: &str) -> Option<(std::path::PathBuf, std::collections::HashMap<String, String>)> {
    let parts: Vec<&str> = target.split('/').filter(|s| !s.is_empty()).collect();
    let mut current_path = base_path.to_path_buf();
    let mut params = std::collections::HashMap::new();

    for (i, part) in parts.iter().enumerate() {
        let mut found = false;
        
        // 1. Try exact match (directory or file)
        let exact = current_path.join(part);
        if exact.exists() {
            current_path = exact;
            found = true;
        } else {
            // 2. Try .erm match
            let erm = current_path.join(format!("{}.erm", part));
            if erm.exists() {
                current_path = erm;
                found = true;
            } else {
                // 3. Try dynamic match
                if let Ok(entries) = fs::read_dir(&current_path) {
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().into_owned();
                        if name.starts_with('[') {
                            if name.ends_with(']') { // Directory [slug]
                                let param_name = &name[1..name.len() - 1];
                                params.insert(param_name.to_string(), part.to_string());
                                current_path.push(name);
                                found = true;
                                break;
                            } else if name.ends_with("].erm") { // File [slug].erm
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
        
        // If we found a file and it's not the last part, we can't go further
        if current_path.is_file() && i < parts.len() - 1 {
            return None;
        }
    }

    if current_path.is_dir() {
        let index_erm = current_path.join("index.erm");
        if index_erm.exists() { return Some((index_erm, params)); }
        let page_erm = current_path.join("page.erm");
        if page_erm.exists() { return Some((page_erm, params)); }
    }

    Some((current_path, params))
}
