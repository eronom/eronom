use std::fs;
use std::path::Path;
use regex::Regex;
use crate::compiler;
use super::commands::{get_port_from_config_file, BuildMode};

#[derive(Debug, Clone)]
pub struct PageRoute {
    pub rel_path: String,
    pub route_path: String,
}

pub fn get_page_route(rel_path: &str) -> Option<PageRoute> {
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
        if *last == "layout.erm" || *last == "loading.erm" {
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
    
    for part in parts.iter() {
        if part.starts_with('[') && part.ends_with(']') {
            let param_name = &part[1..part.len() - 1];
            route_segments.push(format!(":{}", param_name));
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
    })
}

pub fn generate_server_script<F>(routes: &[PageRoute], api_routes: &[String], port: u16, get_render_path: F) -> String
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
    code.push_str(&format!("app.listen({}, () => {{\n  print(\"Server running on port {}\")\n}})\n", port, port));
    code
}

pub fn generate_server_er(routes: &[PageRoute], api_routes: &[String], port: u16) -> String {
    generate_server_script(routes, api_routes, port, |r| r.rel_path.clone())
}

pub fn get_ssg_html_path(rel_path: &str) -> String {
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

pub fn generate_server_er_ssg(routes: &[PageRoute], api_routes: &[String], port: u16) -> String {
    generate_server_script(routes, api_routes, port, |r| get_ssg_html_path(&r.rel_path))
}

pub fn rewrite_api_route_paths(content: &str, prefix: &str) -> String {
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

pub fn build_dir_recursive(
    root: &Path,
    current: &Path,
    build_root: &Path,
    mode: BuildMode,
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
           name_str == "dist" ||
           name_str == "deps" ||
           name_str == "graphify-out" ||
           name_str == "example-er" ||
           name_str == "node_modules" || 
           name_str == "src" ||
           name_str == "libs" ||
           name_str == "external" ||
           name_str == "std" ||
           name_str == "Cargo.toml" ||
           name_str == "Cargo.lock" ||
           name_str == "cargo.log" ||
           name_str == "build.rs" ||
           name_str == "temp_compiled.mir" ||
           name_str == "eronom" ||
           name_str == "LICENSE" ||
           name_str == "README.md" ||
           path.extension().map_or(false, |ext| ext == "rs" || ext == "py" || ext == "log" || ext == "mir" || ext == "exe")
        {
            continue;
        }

        if path.is_dir() {
            build_dir_recursive(root, &path, build_root, mode, routes, api_routes)?;
        } else {
            let rel_path = path.strip_prefix(root)?;
            let dest_path = build_root.join(rel_path);
            
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent)?;
            }

            if name_str.ends_with(".erm") {
                if mode == BuildMode::Ssr {
                    // SSR mode: just copy the .erm file
                    fs::copy(&path, &dest_path)?;
                    if let Some(r) = get_page_route(&rel_path.to_string_lossy()) {
                        routes.push(r);
                    }
                } else {
                    // SSG or PPR mode: compile to .html
                    if name_str == "layout.erm" || name_str == "loading.erm" {
                        continue;
                    }
                    // Skip components (starts with uppercase)
                    if name_str.chars().next().unwrap().is_ascii_uppercase() {
                        continue;
                    }

                    let content = fs::read_to_string(&path)?;
                    let parent_dir = path.parent().unwrap().to_string_lossy();
                    match compiler::process_erm_component(path.to_str().unwrap_or(&parent_dir), &content, true, &std::collections::HashMap::new()) {
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

pub fn build_project(dir: &str, mode: BuildMode) -> anyhow::Result<()> {
    let input_path = Path::new(dir);
    if !input_path.exists() {
        anyhow::bail!("Project directory not found: '{}'", dir);
    }
    if input_path.is_file() {
        anyhow::bail!("Cannot build project from single file '{}'. Use 'eronom build {} --target <target>' to compile a script.", dir, dir);
    }
    let build_dir = input_path.join("build");
    match mode {
        BuildMode::Ssr => println!("Building project for SSR to {:?}", build_dir),
        BuildMode::Ssg => println!("Building project (SSG) to {:?}", build_dir),
        BuildMode::Ppr => println!("Building project for PPR (Partial Prerendering) to {:?}", build_dir),
    }
    
    if build_dir.exists() {
        fs::remove_dir_all(&build_dir)?;
    }
    fs::create_dir_all(&build_dir)?;

    let base_path = fs::canonicalize(dir)?;

    // Parse ermcss config and compile if enabled
    let ermcss_cfg = crate::compiler::parse_ermcss_config(&base_path);
    if ermcss_cfg.enabled {
        println!("[ermcss] Compiling project styles...");
        match crate::compiler::compile_project_ermcss(&base_path, &ermcss_cfg.content) {
            Ok(css) => {
                crate::compiler::set_global_ermcss(css.clone());
                println!("[ermcss] Styles compiled successfully.");
                let css_dir = build_dir.join("css");
                if !css_dir.exists() {
                    fs::create_dir_all(&css_dir)?;
                }
                fs::write(css_dir.join("global.css"), css)?;
            }
            Err(e) => {
                eprintln!("[Warning] Failed to compile global ermcss styles: {}", e);
            }
        }
    } else {
        crate::compiler::set_global_ermcss(String::new());
    }

    let mut routes = Vec::new();
    let mut api_routes = Vec::new();
    build_dir_recursive(&base_path, &base_path, &build_dir, mode, &mut routes, &mut api_routes)?;

    let port = get_port_from_config_file(dir).unwrap_or(3000);

    match mode {
        BuildMode::Ssr => {
            // Write the generated server.er file
            let server_er_content = generate_server_er(&routes, &api_routes, port);
            let server_er_path = build_dir.join("server.er");
            fs::write(server_er_path, server_er_content)?;
        }
        BuildMode::Ssg | BuildMode::Ppr => {
            // Write the generated server.er file for SSG/PPR
            let server_er_content = generate_server_er_ssg(&routes, &api_routes, port);
            let server_er_path = build_dir.join("server.er");
            fs::write(server_er_path, server_er_content)?;
        }
    }

    Ok(())
}
