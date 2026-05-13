use std::path::Path;
use eronom::er;
use eronom::compiler;
use tiny_http::{Server, Response, Header};
use std::fs;

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

        let mut file_path = if target == "/" {
            base_path.join("index.erm")
        } else {
            base_path.join(&target[1..])
        };
        
        if !file_path.exists() && !target.ends_with(".erm") {
            let erm_variant = base_path.join(format!("{}.erm", &target[1..]));
            if erm_variant.exists() {
                file_path = erm_variant;
            }
        }

        if file_path.is_dir() {
            let index_erm = file_path.join("index.erm");
            let page_erm = file_path.join("page.erm");
            if index_erm.exists() {
                file_path = index_erm;
            } else if page_erm.exists() {
                file_path = page_erm;
            }
        }

        if file_path.exists() && file_path.is_file() {
            if file_path.extension().map_or(false, |ext| ext == "erm") {
                let content = fs::read_to_string(&file_path)?;
                let parent = file_path.parent().unwrap().to_string_lossy();
                match compiler::process_erm_component(&parent, &content, is_prod) {
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
