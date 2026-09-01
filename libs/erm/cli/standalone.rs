use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use super::commands::BuildMode;
use super::build::build_project;

pub fn collect_files_recursive(dir: &Path, base: &Path, files: &mut HashMap<String, Vec<u8>>) -> anyhow::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "target" || name == "node_modules" {
            continue;
        }
        if p.is_dir() {
            collect_files_recursive(&p, base, files)?;
        } else if p.is_file() {
            if let Ok(rel) = p.strip_prefix(base) {
                let rel_str = rel.to_string_lossy().replace('\\', "/");
                if let Ok(content) = fs::read(&p) {
                    files.insert(rel_str, content);
                }
            }
        }
    }
    Ok(())
}

pub fn build_standalone(
    dir: &str,
    mode: BuildMode,
    target_str: &str,
    output_opt: Option<String>,
    runner_stub_opt: Option<String>,
) -> anyhow::Result<()> {
    let input_path = Path::new(dir);
    if !input_path.exists() {
        anyhow::bail!("File or directory not found: '{}'", dir);
    }
    let is_single_script = input_path.is_file();
    if dir.ends_with(".er") && !is_single_script {
        anyhow::bail!("'{}' is a directory, expected a .er script file.", dir);
    }

    // Determine target platform
    let target_lower = target_str.to_lowercase();
    let is_windows_target = target_lower == "windows" || target_lower == "win" || target_lower.contains("windows") || target_lower.ends_with(".exe");
    let is_macos_target = target_lower == "macos" || target_lower == "mac" || target_lower == "darwin" || target_lower.contains("apple");
    let is_linux_target = target_lower == "linux" || target_lower.contains("linux");
    
    let target_display = if is_windows_target {
        "Windows (x86_64-pc-windows-msvc)"
    } else if is_macos_target {
        "macOS (universal / darwin)"
    } else if is_linux_target {
        "Linux (x86_64-unknown-linux-gnu)"
    } else {
        "Host Native"
    };

    let mut bundle_files = HashMap::new();
    let entrypoint: String;
    let bundle_mode: String;
    let default_stem: String;

    if is_single_script {
        let script_file = fs::canonicalize(input_path)?;
        let file_name = script_file.file_name().unwrap_or_default().to_string_lossy().to_string();
        default_stem = script_file.file_stem().unwrap_or_default().to_string_lossy().to_string();
        entrypoint = file_name.clone();
        bundle_mode = "single_script".to_string();

        let script_bytes = fs::read(&script_file)?;
        bundle_files.insert(file_name.clone(), script_bytes);

        // Also recursively gather any sibling .er files imported if in same directory
        if let Some(parent) = script_file.parent() {
            if let Ok(entries) = fs::read_dir(parent) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_file() && p.extension().map_or(false, |e| e == "er") {
                        let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
                        if !bundle_files.contains_key(&name) {
                            if let Ok(data) = fs::read(&p) {
                                bundle_files.insert(name, data);
                            }
                        }
                    }
                }
            }
        }
    } else {
        // Build ERM project first
        build_project(dir, mode)?;

        let base_path = fs::canonicalize(dir)?;
        let build_dir = base_path.join("build");
        default_stem = base_path.file_name().unwrap_or_default().to_string_lossy().to_string();
        entrypoint = "server.er".to_string();
        bundle_mode = "erm_app".to_string();

        // Collect all build artifacts (server.er, compiled HTML/ERM, css, pages, etc.)
        collect_files_recursive(&build_dir, &build_dir, &mut bundle_files)?;

        // Collect eronom.toml if present
        let toml_path = base_path.join("eronom.toml");
        if toml_path.exists() {
            if let Ok(content) = fs::read(&toml_path) {
                bundle_files.insert("eronom.toml".to_string(), content);
            }
        }

        // Collect static assets or public folder if present
        let public_dir = base_path.join("public");
        if public_dir.exists() {
            collect_files_recursive(&public_dir, &public_dir, &mut bundle_files)?;
        }
    }

    // Always bundle standard library files
    let std_files = crate::vm::embedded::collect_std_library_files();
    for (k, v) in std_files {
        bundle_files.insert(k, v);
    }

    let mut bundle = crate::vm::embedded::EmbeddedBundle::new(&entrypoint, &bundle_mode);
    for (path, data) in bundle_files {
        bundle.add_file(&path, data);
    }

    // Determine output path
    let output_path = if let Some(out_str) = output_opt {
        let mut p = PathBuf::from(out_str);
        if is_windows_target && p.extension().map_or(true, |e| e != "exe") {
            p.set_extension("exe");
        }
        p
    } else {
        let dist_dir = PathBuf::from("dist");
        let bin_name = if is_windows_target {
            format!("{}.exe", default_stem)
        } else {
            default_stem
        };
        dist_dir.join(bin_name)
    };

    // Obtain runner stub binary
    let runner_bytes = if let Some(stub_path) = runner_stub_opt {
        let stub_p = Path::new(&stub_path);
        if !stub_p.exists() {
            anyhow::bail!("Specified runner stub not found: {}", stub_path);
        }
        fs::read(stub_p)?
    } else {
        // Check for target runner in ~/.eronom/runners/
        let mut found_stub = None;
        if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
            let runners_dir = Path::new(&home).join(".eronom").join("runners");
            let target_names = if is_windows_target {
                vec!["runner-windows.exe", "runner-x86_64-pc-windows-msvc.exe", "eronom.exe"]
            } else if is_macos_target {
                vec!["runner-macos", "runner-aarch64-apple-darwin", "runner-x86_64-apple-darwin"]
            } else if is_linux_target {
                vec!["runner-linux", "runner-x86_64-unknown-linux-gnu", "eronom"]
            } else {
                vec![]
            };
            for name in target_names {
                let candidate = runners_dir.join(name);
                if candidate.exists() {
                    if let Ok(b) = fs::read(&candidate) {
                        found_stub = Some(b);
                        break;
                    }
                }
            }
        }

        if let Some(b) = found_stub {
            b
        } else {
            // Use current executable
            let current_exe = std::env::current_exe()?;
            fs::read(&current_exe)?
        }
    };

    println!("\n⚡ Eronom Standalone Compiler");
    println!("  ├─ Mode:            {} ({})", bundle_mode, entrypoint);
    println!("  ├─ Target Platform: {}", target_display);
    println!("  ├─ Embedded Files:  {} files", bundle.files.len());

    crate::vm::embedded::build_standalone_executable(&runner_bytes, &bundle, &output_path)?;

    let bin_size = fs::metadata(&output_path)?.len();
    let bin_size_mb = (bin_size as f64) / (1024.0 * 1024.0);

    println!("  ├─ Output Binary:   {}", output_path.display());
    println!("  └─ Binary Size:     {:.2} MB", bin_size_mb);
    println!("\n✓ Standalone binary created successfully!");
    println!("  Run executable with: ./{}", output_path.display());

    Ok(())
}
