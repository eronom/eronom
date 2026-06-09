use std::path::Path;
use regex::Regex;
use crate::compiler;
use crate::server::start_server;
use std::fs;

pub fn run_cli(args: Vec<String>) -> anyhow::Result<()> {
    let mut cmd = "dev";
    let mut dir = ".";
    let mut port_val: Option<u16> = None;
    let mut is_ssr = true;
    let mut has_custom_port = false;

    if args.len() > 1 {
        let first_arg = &args[1];
        if matches!(first_arg.as_str(), "build" | "dev" | "start" | "init") {
            cmd = first_arg;
            
            let mut pos_args = Vec::new();
            for arg in args.iter().skip(2) {
                if arg == "--ssg" || arg == "-ssg" {
                    is_ssr = false;
                } else if arg == "--ssr" || arg == "-ssr" {
                    is_ssr = true;
                } else if arg.starts_with('-') {
                    // ignore unknown flags for now
                } else {
                    pos_args.push(arg);
                }
            }
            
            if cmd == "start" {
                dir = "build";
            }
            if !pos_args.is_empty() {
                if cmd == "start" && pos_args[0].parse::<u16>().is_ok() {
                    let p = pos_args[0].parse::<u16>().unwrap();
                    port_val = Some(p);
                    has_custom_port = true;
                } else {
                    dir = pos_args[0];
                    if pos_args.len() > 1 && cmd != "build" {
                        match pos_args[1].parse::<u16>() {
                            Ok(p) => {
                                port_val = Some(p);
                                has_custom_port = true;
                            }
                            Err(_) => {
                                anyhow::bail!("Invalid port number: '{}'", pos_args[1]);
                            }
                        }
                    }
                }
            }
        } else {
            dir = first_arg;
        }
    }


    if dir == "build" && !Path::new("build").exists() && Path::new("app/build").exists() {
        dir = "app/build";
    }

    let port = if has_custom_port {
        port_val.unwrap()
    } else if let Ok(port_str) = std::env::var("PORT") {
        if let Ok(p) = port_str.parse::<u16>() {
            p
        } else {
            anyhow::bail!("Invalid port in PORT environment variable: '{}'", port_str);
        }
    } else if let Some(config_port) = get_port_from_config_file(dir) {
        config_port
    } else if cmd == "build" || cmd == "init" {
        0
    } else {
        anyhow::bail!("No port specified. Please provide a port in config.er or PORT environment variable.");
    };

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

    let dst_dir = Path::new(dir);

    // Locate the template init directory
    let mut src_init = std::path::PathBuf::from("libs/init");
    if !src_init.exists() {
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let sibling_init = exe_dir.join("libs/init");
                if sibling_init.exists() {
                    src_init = sibling_init;
                } else if let Some(parent_dir) = exe_dir.parent() {
                    let parent_init = parent_dir.join("libs/init");
                    if parent_init.exists() {
                        src_init = parent_init;
                    }
                }
            }
        }
    }

    if !src_init.exists() {
        anyhow::bail!("Source 'libs/init' directory not found. Please run 'eronom init' from the directory containing the 'libs/init' template.");
    }

    println!("Copying template from {} to {}", src_init.display(), dst_dir.display());
    copy_dir_all(&src_init, dst_dir)?;

    println!("Fresh Eronom project initialized successfully under {}", dst_dir.display());
    Ok(())
}

fn copy_dir_all(src: &Path, dst: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let file_name = entry.file_name();
        
        // Exclude the compiled eronom binary if it is in the source folder
        if file_name == "eronom" {
            continue;
        }

        let dst_path = dst.join(&file_name);
        if file_type.is_dir() {
            copy_dir_all(&entry.path(), &dst_path)?;
        } else {
            fs::copy(&entry.path(), &dst_path)?;
        }
    }
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
    
    if let Some(pages_idx) = parts.iter().position(|&s| s == "pages") {
        parts = parts[(pages_idx + 1)..].to_vec();
    } else {
        return None;
    }
    
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

fn generate_server_script<F>(routes: &[PageRoute], api_routes: &[String], get_render_path: F) -> String
where
    F: Fn(&PageRoute) -> String,
{
    let mut sorted_routes = routes.to_vec();
    sorted_routes.sort_by(|a, b| {
        if a.route_path == "/" {
            std::cmp::Ordering::Less
        } else if b.route_path == "/" {
            std::cmp::Ordering::Greater
        } else {
            a.route_path.cmp(&b.route_path)
        }
    });

    let mut code = String::new();
    for api in api_routes {
        code.push_str(&format!("import \"./{}\"\n", api.replace('\\', "/")));
    }
    if !api_routes.is_empty() {
        code.push_str("\n");
    }

    code.push_str("import { router } from \"std/http\"\n\nlet app = router()\n\n");
    for route in &sorted_routes {
        let render_path = get_render_path(route);
        let route_block = format!(
            r#"app.get("{}", (c) => {{
  let template = render("{}", c.req.params)
  return c.html(template)
}})"#,
            route.route_path, render_path
        );
        code.push_str(&route_block);
        code.push_str("\n\n");
    }
    code
}

fn generate_server_er(routes: &[PageRoute], api_routes: &[String]) -> String {
    generate_server_script(routes, api_routes, |r| r.rel_path.clone())
}

fn get_ssg_html_path(rel_path: &str) -> String {
    let path = Path::new(rel_path);
    let name_str = path.file_name().unwrap_or_default().to_string_lossy();
    let mut dest_path = path.to_path_buf();
    if name_str == "page.erm" || name_str == "index.erm" {
        dest_path.set_file_name("index.html");
    } else {
        dest_path.set_extension("html");
    }
    dest_path.to_string_lossy().replace("\\", "/")
}

fn generate_server_er_ssg(routes: &[PageRoute], api_routes: &[String]) -> String {
    generate_server_script(routes, api_routes, |r| get_ssg_html_path(&r.rel_path))
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
    let mut api_routes = Vec::new();
    build_dir_recursive(&base_path, &base_path, &build_dir, is_ssr, &mut routes, &mut api_routes)?;

    if is_ssr {
        // Write the generated server.er file
        let server_er_content = generate_server_er(&routes, &api_routes);
        let server_er_path = build_dir.join("server.er");
        fs::write(server_er_path, server_er_content)?;
    } else {
        // Write the generated server.er file for SSG
        let server_er_content = generate_server_er_ssg(&routes, &api_routes);
        let server_er_path = build_dir.join("server.er");
        fs::write(server_er_path, server_er_content)?;
    }

    Ok(())
}

fn build_dir_recursive(
    root: &Path,
    current: &Path,
    build_root: &Path,
    is_ssr: bool,
    routes: &mut Vec<PageRoute>,
    api_routes: &mut Vec<String>,
) -> anyhow::Result<()> {
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
            build_dir_recursive(root, &path, build_root, is_ssr, routes, api_routes)?;
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
                            if let Some(r) = get_page_route(&rel_path.to_string_lossy()) {
                                routes.push(r);
                            }
                        }
                        Err(e) => {
                            eprintln!("Error compiling {}: {}", path.display(), e);
                        }
                    }
                }
            } else if name_str.ends_with(".er") {
                let rel_path_str = rel_path.to_string_lossy().replace('\\', "/");
                if rel_path_str.starts_with("server/api/") {
                    let content = fs::read_to_string(&path)?;
                    let parent = rel_path.parent().unwrap_or(Path::new(""));
                    let mut parent_str = parent.to_string_lossy().replace('\\', "/");
                    if parent_str.starts_with("server/") {
                        parent_str = parent_str["server/".len()..].to_string();
                    }
                    let prefix = if parent_str.is_empty() {
                        String::new()
                    } else {
                        format!("/{}", parent_str)
                    };
                    
                    let rewritten = rewrite_api_route_paths(&content, &prefix);
                    fs::write(&dest_path, rewritten)?;
                    api_routes.push(rel_path.to_string_lossy().into_owned());
                } else {
                    fs::copy(&path, &dest_path)?;
                }
            } else {
                // Copy assets
                fs::copy(&path, &dest_path)?;
            }
        }
    }
    Ok(())
}

fn rewrite_api_route_paths(content: &str, prefix: &str) -> String {
    let re = Regex::new(r#"(?x)
        app\s*\.\s*(get|post|put|delete|patch|ws)\s*\(\s*(['"])([^'"]*)(['"])
    "#).unwrap();
    
    re.replace_all(content, |caps: &regex::Captures| {
        let method = &caps[1];
        let quote = &caps[2];
        let path = &caps[3];
        
        let new_path = if path == "/" {
            prefix.to_string()
        } else if path.starts_with('/') {
            format!("{}{}", prefix, path)
        } else {
            format!("{}/{}", prefix, path)
        };
        
        format!("app.{}({}{}{}", method, quote, new_path, quote)
    }).into_owned()
}


fn get_port_from_config_file(dir: &str) -> Option<u16> {
    let path = Path::new(dir);
    let mut config_path = if path.is_file() {
        path.parent()?.join("config.er")
    } else {
        path.join("config.er")
    };

    let mut current = config_path.parent();
    while let Some(p) = current {
        let check = p.join("config.er");
        if check.exists() {
            config_path = check;
            break;
        }
        current = p.parent();
    }

    if !config_path.exists() {
        return None;
    }

    let content = fs::read_to_string(config_path).ok()?;
    let re = regex::Regex::new(r"(?s)server\s*:\s*\{[^}]*port\s*:\s*(\d+)").ok()?;
    let caps = re.captures(&content)?;
    caps.get(1)?.as_str().parse::<u16>().ok()
}
