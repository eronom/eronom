use std::path::Path;
use crate::er;
use crate::compiler;
use crate::server::start_server;
use std::fs;

pub fn run_cli(args: Vec<String>) -> anyhow::Result<()> {
    let mut cmd = "dev";
    let mut dir = ".";
    let mut port: u16 = 3000;
    let mut is_ssr = false;
    let mut has_custom_port = false;

    if args.len() > 1 {
        let first_arg = &args[1];
        if matches!(first_arg.as_str(), "build" | "dev" | "start" | "init") {
            cmd = first_arg;
            
            let mut pos_args = Vec::new();
            for arg in args.iter().skip(2) {
                if arg == "--ssr" || arg == "-ssr" {
                    is_ssr = true;
                } else if arg.starts_with("--port=") {
                    port = arg[7..].parse().unwrap_or(3000);
                    has_custom_port = true;
                } else if arg.starts_with('-') {
                    // ignore unknown flags for now
                } else {
                    pos_args.push(arg);
                }
            }
            
            if !pos_args.is_empty() {
                dir = pos_args[0];
                if pos_args.len() > 1 && cmd != "build" {
                    port = pos_args[1].parse().unwrap_or(3000);
                    has_custom_port = true;
                }
            } else if cmd == "start" {
                dir = "build";
            }
        } else if first_arg.ends_with(".er") {
            er::run_file(first_arg)?;
            return Ok(());
        } else {
            dir = first_arg;
        }
    }

    if !has_custom_port {
        if let Some(config_port) = get_port_from_config_file(dir) {
            port = config_port;
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

#[derive(Debug, Clone)]
struct PageRoute {
    rel_path: String,
    route_path: String,
    params: Vec<(String, usize)>,
}

fn get_page_route(rel_path: &str) -> Option<PageRoute> {
    let path_str = rel_path.replace("\\", "/");
    let mut parts: Vec<&str> = path_str.split('/').collect();
    
    // Check if filename starts with uppercase (it's a component, not a page)
    if let Some(last) = parts.last() {
        if last.chars().next().map_or(false, |c| c.is_ascii_uppercase()) {
            return None;
        }
        if *last == "layout.erm" {
            return None;
        }
    }
    
    // Remove extension from last part
    if let Some(last) = parts.last_mut() {
        if last.ends_with(".erm") {
            *last = &last[..last.len() - 4];
        }
    }
    
    // Handle index / page
    if let Some(last) = parts.last() {
        if *last == "index" || *last == "page" {
            parts.pop();
        }
    }
    
    let mut route_segments = Vec::new();
    let mut params = Vec::new();
    
    for (i, part) in parts.iter().enumerate() {
        if part.starts_with('[') && part.ends_with(']') {
            let param_name = &part[1..part.len() - 1];
            route_segments.push(format!(":{}", param_name));
            // Index in url.split("/") will be i + 1
            params.push((param_name.to_string(), i + 1));
        } else {
            route_segments.push(part.to_string());
        }
    }
    
    let route_path = if route_segments.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", route_segments.join("/"))
    };
    
    Some(PageRoute {
        rel_path: path_str,
        route_path,
        params,
    })
}

fn generate_server_er(routes: &[PageRoute]) -> String {
    let mut code = String::new();
    code.push_str("import { route } from \"std/http\"\n\n");
    code.push_str("let app = route()\n\n");
    
    for route in routes {
        code.push_str(&format!("app.get(\"{}\", (c) => {{\n", route.route_path));
        code.push_str(&format!("  let html = renderErm(\"{}\", c.req.params)\n", route.rel_path));
        code.push_str("  return c.html(html)\n");
        code.push_str("})\n\n");
    }
    
    code
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
    let mut routes = Vec::new();
    build_dir_recursive(&base_path, &base_path, &build_dir, is_ssr, &mut routes)?;

    if is_ssr {
        // Write the generated server.er file
        let server_er_content = generate_server_er(&routes);
        let server_er_path = build_dir.join("server.er");
        fs::write(server_er_path, server_er_content)?;
    }

    Ok(())
}

fn build_dir_recursive(root: &Path, current: &Path, build_root: &Path, is_ssr: bool, routes: &mut Vec<PageRoute>) -> anyhow::Result<()> {
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
           name_str == "libs" ||
           name_str == "external" ||
           name_str == "std" ||
           name_str == "Cargo.toml" ||
           name_str == "Cargo.lock" ||
           name_str == "cargo.log" ||
           name_str == "build.rs" ||
           name_str == "benchmark_ws.py" ||
           name_str == "temp_compiled.mir" ||
           name_str == "eronom" ||
           name_str == "LICENSE" ||
           name_str == "README.md" ||
           path.extension().map_or(false, |ext| ext == "rs" || ext == "py" || ext == "log" || ext == "mir")
        {
            continue;
        }

        if path.is_dir() {
            build_dir_recursive(root, &path, build_root, is_ssr, routes)?;
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
                    if let Some(r) = get_page_route(&rel_path.to_string_lossy()) {
                        routes.push(r);
                    }
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


fn get_port_from_config_file(dir: &str) -> Option<u16> {
    let path = Path::new(dir);
    let config_path = if path.is_file() {
        path.parent()?.join("config.er")
    } else {
        path.join("config.er")
    };

    if !config_path.exists() {
        return None;
    }

    let content = fs::read_to_string(config_path).ok()?;
    let re = regex::Regex::new(r"(?s)server\s*:\s*\{[^}]*port\s*:\s*(\d+)").ok()?;
    let caps = re.captures(&content)?;
    caps.get(1)?.as_str().parse::<u16>().ok()
}
