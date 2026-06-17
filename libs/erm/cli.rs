use std::path::{Path, PathBuf};
use regex::Regex;
use crate::compiler;
use crate::server::start_server;
use std::fs;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "eronom", version = "0.1.0")]
#[command(about = "Eronom: A web framework and runtime", long_about = None)]
pub struct Cli {
    /// The script file to run (e.g. main.er)
    pub file: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Initialize a fresh Eronom project
    Init {
        /// The root directory of the new project
        #[arg(default_value = ".")]
        dir: String,

        /// The template to start from
        #[arg(long, short)]
        template: Option<String>,

        /// Branch argument that can only be used with template option
        #[arg(long, short, requires = "template")]
        branch: Option<String>,

        /// Create the project even if the specified directory is not empty
        #[arg(long)]
        force: bool,

        /// Do not initialize a git repository
        #[arg(long)]
        no_git: bool,

        /// Do not create an initial commit
        #[arg(long)]
        no_commit: bool,
    },
    /// Build the project
    Build {
        /// Source directory
        #[arg(default_value = ".")]
        dir: String,

        /// Build for Server-Side Rendering (SSR) (default)
        #[arg(long, conflicts_with = "ssg")]
        ssr: bool,

        /// Build for Static Site Generation (SSG)
        #[arg(long)]
        ssg: bool,
    },
    /// Start the server in production mode
    Start {
        /// Build directory or port
        dir_or_port: Option<String>,

        /// Port number if directory was also specified
        port_pos: Option<u16>,

        /// Port number
        #[arg(short, long = "port")]
        port: Option<u16>,
    },
    /// Start the server in development mode
    Dev {
        /// Source directory or port
        dir_or_port: Option<String>,

        /// Port number if directory was also specified
        port_pos: Option<u16>,

        /// Port number
        #[arg(short, long = "port")]
        port: Option<u16>,
    },
}

pub fn run_cli(args: Vec<String>) -> anyhow::Result<()> {
    let cli = Cli::try_parse_from(args)?;
    if let Some(cmd) = cli.command {
        run_command(cmd)
    } else if let Some(file_path) = cli.file {
        anyhow::bail!("Running files is handled via the main entrypoint: {}", file_path.display())
    } else {
        anyhow::bail!("No command or file specified")
    }
}

fn parse_dir_and_port(
    dir_or_port: Option<String>,
    port_pos: Option<u16>,
    port_flag: Option<u16>,
    default_dir: &str,
) -> anyhow::Result<(String, Option<u16>)> {
    let mut dir = default_dir.to_string();
    let mut port_val = port_flag;

    if let Some(arg) = dir_or_port {
        if let Ok(p) = arg.parse::<u16>() {
            if port_val.is_none() {
                port_val = Some(p);
            }
        } else {
            dir = arg;
            if port_val.is_none() {
                port_val = port_pos;
            }
        }
    }
    Ok((dir, port_val))
}

fn resolve_port(dir: &str, port_val: Option<u16>, is_build_or_init: bool) -> anyhow::Result<u16> {
    if let Some(p) = port_val {
        Ok(p)
    } else if let Ok(port_str) = std::env::var("PORT") {
        if let Ok(p) = port_str.parse::<u16>() {
            Ok(p)
        } else {
            anyhow::bail!("Invalid port in PORT environment variable: '{}'", port_str);
        }
    } else if let Some(config_port) = get_port_from_config_file(dir) {
        Ok(config_port)
    } else if is_build_or_init {
        Ok(0)
    } else {
        anyhow::bail!("No port specified. Please provide a port in config.er or PORT environment variable.");
    }
}

pub fn run_command(cmd: Commands) -> anyhow::Result<()> {
    match cmd {
        Commands::Init {
            dir,
            template,
            branch,
            force,
            no_git,
            no_commit,
        } => {
            init_project(&dir, template, branch, force, no_git, no_commit)?;
        }
        Commands::Build { dir, ssr: _, ssg } => {
            let is_ssr = if ssg { false } else { true };
            build_project(&dir, is_ssr)?;
        }
        Commands::Start {
            dir_or_port,
            port_pos,
            port,
        } => {
            let (mut dir, port_val) = parse_dir_and_port(dir_or_port, port_pos, port, "build")?;
            if dir == "build" && !Path::new("build").exists() && Path::new("app/build").exists() {
                dir = "app/build".to_string();
            }
            let resolved_port = resolve_port(&dir, port_val, false)?;
            start_server(&dir, true, resolved_port)?;
        }
        Commands::Dev {
            dir_or_port,
            port_pos,
            port,
        } => {
            let (dir, port_val) = parse_dir_and_port(dir_or_port, port_pos, port, ".")?;
            let resolved_port = resolve_port(&dir, port_val, false)?;
            start_server(&dir, false, resolved_port)?;
        }
    }
    Ok(())
}


fn is_dir_empty(path: &Path) -> bool {
    if let Ok(mut entries) = fs::read_dir(path) {
        entries.next().is_none()
    } else {
        true
    }
}

fn init_project(
    dir: &str,
    template: Option<String>,
    branch: Option<String>,
    force: bool,
    no_git: bool,
    no_commit: bool,
) -> anyhow::Result<()> {
    let dst_dir = Path::new(dir);

    if dst_dir.exists() && !is_dir_empty(dst_dir) && !force {
        anyhow::bail!(
            "Cannot run `init` on a non-empty directory.\n\
             Run with the `--force` flag to initialize regardless."
        );
    }

    fs::create_dir_all(dst_dir)?;

    if let Some(template_str) = template.as_ref() {
        let template_url = if template_str.contains("://") {
            template_str.clone()
        } else if template_str.starts_with("github.com/") {
            format!("https://{}", template_str)
        } else {
            format!("https://github.com/{}", template_str)
        };

        let temp_dir_name = format!(
            "eronom-template-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        );
        let temp_dir = std::env::temp_dir().join(temp_dir_name);

        println!("Cloning template from {}...", template_url);
        let mut git_clone = std::process::Command::new("git");
        git_clone.arg("clone").arg("--depth").arg("1");
        if let Some(ref b) = branch {
            git_clone.arg("-b").arg(b);
        }
        git_clone.arg(&template_url).arg(&temp_dir);

        let status = git_clone.status()?;
        if !status.success() {
            anyhow::bail!("Failed to clone template from {}", template_url);
        }

        println!("Copying template files...");
        copy_dir_all(&temp_dir, dst_dir)?;
        let _ = fs::remove_dir_all(&temp_dir);
    }

    // Copy the std library directory to dst_dir/std
    let dest_std = dst_dir.join("std");
    println!("Installing std in {} (url: https://github.com/eronom/eronom/tree/main/std)", dest_std.display());
    println!("Cloning into '{}'...", dest_std.display());

    let mut success = false;
    let mut commit_hash = String::new();

    let temp_dir_name = format!(
        "eronom-std-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );
    let temp_dir = std::env::temp_dir().join(temp_dir_name);
    let mut git_clone = std::process::Command::new("git");
    git_clone.arg("clone")
        .arg("--depth").arg("1")
        .arg("https://github.com/eronom/eronom.git")
        .arg(&temp_dir);
    
    if let Ok(status) = git_clone.status() {
        if status.success() {
            let repo_std = temp_dir.join("std");
            if repo_std.exists() {
                if copy_dir_all(&repo_std, &dest_std).is_ok() {
                    success = true;
                    // Get commit hash
                    let mut git_rev = std::process::Command::new("git");
                    git_rev.arg("rev-parse").arg("HEAD").current_dir(&temp_dir);
                    if let Ok(output) = git_rev.output() {
                        if output.status.success() {
                            commit_hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
                        }
                    }
                }
            }
            let _ = fs::remove_dir_all(&temp_dir);
        }
    }

    if success {
        if !commit_hash.is_empty() {
            println!("    Installed std commit={}", commit_hash);
        } else {
            println!("    Installed std");
        }
    } else {
        // Fallback to local copy if clone failed (e.g. offline)
        let mut std_src = std::path::PathBuf::from("std");
        if !std_src.exists() {
            if let Ok(exe_path) = std::env::current_exe() {
                if let Some(exe_dir) = exe_path.parent() {
                    let sibling_std = exe_dir.join("std");
                    if sibling_std.exists() {
                        std_src = sibling_std;
                    } else if let Some(parent_dir) = exe_dir.parent() {
                        let parent_std = parent_dir.join("std");
                        if parent_std.exists() {
                            std_src = parent_std;
                        }
                    }
                }
            }
        }

        if std_src.exists() && std_src.is_dir() {
            println!("GitHub clone failed or offline. Falling back to local standard library from {}...", std_src.display());
            copy_dir_all(&std_src, &dest_std)?;
            println!("    Installed std (local fallback)");
        } else {
            anyhow::bail!("Failed to clone std library from https://github.com/eronom/eronom.git and no local standard library found.");
        }
    }



    if !no_git {
        // Initialize git repo if not already inside one, or if we want a fresh repo
        println!("Initializing git repository...");
        let mut git_init = std::process::Command::new("git");
        git_init.arg("init")
            .arg("-b").arg("main")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .current_dir(dst_dir);
        let init_status = git_init.status();
        
        if init_status.is_ok() && init_status.unwrap().success() {
            if !no_commit {
                let mut git_add = std::process::Command::new("git");
                git_add.arg("add").arg("-A")
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .current_dir(dst_dir);
                let _ = git_add.status();

                let mut git_commit = std::process::Command::new("git");
                let commit_msg = if let Some(ref t) = template {
                    format!("chore: init from {}", t)
                } else {
                    "chore: eronom init".to_string()
                };
                git_commit.arg("commit").arg("-m").arg(commit_msg)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .current_dir(dst_dir);
                let _ = git_commit.status();
            }
        }
    }


    println!("Fresh Eronom project initialized successfully under {}", dst_dir.display());
    Ok(())
}

fn copy_dir_all(src: &Path, dst: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let file_name = entry.file_name();
        
        // Exclude the compiled eronom binary and git directory if they are in the source folder
        if file_name == "eronom" || file_name == ".git" {
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
