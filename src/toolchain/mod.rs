use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct ToolchainInfo {
    pub name: String,
    pub path: Option<PathBuf>,
    pub installed: bool,
    pub version: Option<String>,
}

/// Find a toolchain executable by name (e.g. "cranelift" -> "eronom-cranelift")
pub fn find_toolchain(name: &str) -> Option<PathBuf> {
    let bin_name = if cfg!(windows) {
        format!("eronom-{}.exe", name)
    } else {
        format!("eronom-{}", name)
    };

    // 1. Check ~/.eronom/toolchains/bin/
    if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
        let p = PathBuf::from(&home).join(".eronom").join("toolchains").join("bin").join(&bin_name);
        if p.exists() {
            return Some(p);
        }
    }

    // 2. Check next to current executable
    if let Ok(exe) = std::env::current_exe() {
        if let Ok(canonical) = exe.canonicalize() {
            if let Some(parent) = canonical.parent() {
                let candidate = parent.join(&bin_name);
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
        if let Some(parent) = exe.parent() {
            let candidate = parent.join(&bin_name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    // 3. Check system PATH
    if let Ok(path_var) = std::env::var("PATH") {
        let sep = if cfg!(windows) { ';' } else { ':' };
        for dir in path_var.split(sep) {
            let candidate = Path::new(dir).join(&bin_name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    // 4. Check workspace target directories (target/debug, target/release, target/<target>/...)
    let target_dirs = [
        "target/debug",
        "target/release",
        "../target/debug",
        "../target/release",
        "target/x86_64-unknown-linux-gnu/debug",
        "target/x86_64-pc-windows-msvc/debug",
    ];

    for td in &target_dirs {
        let p = PathBuf::from(td).join(&bin_name);
        if p.exists() {
            return Some(p);
        }
    }

    // 5. Walk ancestors of CWD looking for target/debug/eronom-<name>
    if let Ok(cwd) = std::env::current_dir() {
        let mut curr = Some(cwd.as_path());
        while let Some(parent) = curr {
            let d1 = parent.join("target").join("debug").join(&bin_name);
            if d1.exists() {
                return Some(d1);
            }
            let d2 = parent.join("target").join("release").join(&bin_name);
            if d2.exists() {
                return Some(d2);
            }
            curr = parent.parent();
        }
    }

    None
}

/// Ensure the specified toolchain is available, building or installing on demand if missing
pub fn ensure_toolchain(name: &str) -> anyhow::Result<PathBuf> {
    if let Some(p) = find_toolchain(name) {
        return Ok(p);
    }

    // If running inside the Eronom development repository, auto-build the workspace toolchain crate
    let is_workspace_repo = check_workspace_repo(name);
    if is_workspace_repo {
        println!("ℹ Toolchain 'eronom-{}' not found in target/. Building it now...", name);
        let status = Command::new("cargo")
            .arg("build")
            .arg("-p")
            .arg(format!("eronom-{}", name))
            .status();

        if let Ok(s) = status {
            if s.success() {
                if let Some(p) = find_toolchain(name) {
                    return Ok(p);
                }
            }
        }
    }

    // In production / standalone usage:
    install_toolchain(name)?;
    if let Some(p) = find_toolchain(name) {
        return Ok(p);
    }

    anyhow::bail!(
        "Toolchain 'eronom-{}' is required but could not be located or installed.\n\
         Try running: eronom toolchain add {}",
        name,
        name
    )
}

fn check_workspace_repo(name: &str) -> bool {
    // Check if `crates/eronom-<name>/Cargo.toml` exists relative to CWD or current_exe
    let crate_rel = format!("crates/eronom-{}/Cargo.toml", name);
    if Path::new(&crate_rel).exists() {
        return true;
    }

    if let Ok(cwd) = std::env::current_dir() {
        let mut curr = Some(cwd.as_path());
        while let Some(parent) = curr {
            if parent.join(&crate_rel).exists() {
                return true;
            }
            curr = parent.parent();
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        let mut curr = exe.parent();
        while let Some(parent) = curr {
            if parent.join(&crate_rel).exists() {
                return true;
            }
            curr = parent.parent();
        }
    }

    false
}

/// Execute a Cranelift AOT compilation using the decoupled `eronom-cranelift` toolchain binary
pub fn run_cranelift_build(script: &Path, output: &Path, target: Option<&str>) -> anyhow::Result<()> {
    let toolchain_bin = ensure_toolchain("cranelift")?;

    let mut cmd = Command::new(&toolchain_bin);
    cmd.arg("compile")
        .arg(script)
        .arg("-o")
        .arg(output);

    if let Some(t) = target {
        cmd.arg("--target").arg(t);
    }

    let status = cmd.status().map_err(|e| {
        anyhow::anyhow!("Failed to invoke eronom-cranelift toolchain at {}: {}", toolchain_bin.display(), e)
    })?;

    if !status.success() {
        anyhow::bail!("Cranelift build failed with exit code: {:?}", status.code());
    }

    Ok(())
}

/// List available and installed Eronom toolchains
pub fn list_toolchains() -> Vec<ToolchainInfo> {
    let known_toolchains = ["cranelift"];
    let mut list = Vec::new();

    for name in &known_toolchains {
        let p = find_toolchain(name);
        let installed = p.is_some();
        let version = p.as_ref().and_then(|bin| {
            Command::new(bin)
                .arg("version")
                .output()
                .ok()
                .and_then(|out| String::from_utf8(out.stdout).ok())
                .map(|s| s.trim().to_string())
        });

        list.push(ToolchainInfo {
            name: name.to_string(),
            path: p,
            installed,
            version,
        });
    }

    list
}

/// Install an optional toolchain to ~/.eronom/toolchains/bin/
pub fn install_toolchain(name: &str) -> anyhow::Result<()> {
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE"))?;
    let bin_dir = PathBuf::from(&home).join(".eronom").join("toolchains").join("bin");
    std::fs::create_dir_all(&bin_dir)?;

    let bin_name = if cfg!(windows) {
        format!("eronom-{}.exe", name)
    } else {
        format!("eronom-{}", name)
    };
    let dest_file = bin_dir.join(&bin_name);

    if check_workspace_repo(name) {
        println!("ℹ Compiling eronom-{} toolchain from local workspace source...", name);
        let status = Command::new("cargo")
            .arg("build")
            .arg("--release")
            .arg("-p")
            .arg(format!("eronom-{}", name))
            .status()?;

        if status.success() {
            let built_bin = PathBuf::from("target").join("release").join(&bin_name);
            if built_bin.exists() {
                std::fs::copy(&built_bin, &dest_file)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&dest_file, std::fs::Permissions::from_mode(0o755));
                }
                println!("✓ Successfully installed eronom-{} to {}", name, dest_file.display());
                return Ok(());
            }
        }
    }

    // Production remote install fallback (e.g. from GitHub releases)
    let host_triple = if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        "x86_64-linux"
    } else if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
        "x86_64-windows.exe"
    } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        "aarch64-macos"
    } else {
        "unknown"
    };

    println!("ℹ Downloading pre-built eronom-{} ({}) from releases...", name, host_triple);
    let download_url = format!(
        "https://github.com/eronom/eronom/releases/latest/download/eronom-{}-{}",
        name, host_triple
    );

    match ureq::get(&download_url).call() {
        Ok(mut resp) => {
            let bytes = resp.body_mut().read_to_vec()?;
            std::fs::write(&dest_file, &bytes)?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&dest_file, std::fs::Permissions::from_mode(0o755));
            }
            println!("✓ Installed eronom-{} to {}", name, dest_file.display());
            Ok(())
        }
        Err(e) => {
            anyhow::bail!(
                "Failed to download pre-built toolchain from {}: {}\n\
                 You can compile it locally with: cargo build -p eronom-{}",
                download_url,
                e,
                name
            )
        }
    }
}

/// Remove an installed toolchain from ~/.eronom/toolchains/bin/
pub fn remove_toolchain(name: &str) -> anyhow::Result<()> {
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE"))?;
    let bin_name = if cfg!(windows) {
        format!("eronom-{}.exe", name)
    } else {
        format!("eronom-{}", name)
    };
    let dest_file = PathBuf::from(&home).join(".eronom").join("toolchains").join("bin").join(&bin_name);

    if dest_file.exists() {
        std::fs::remove_file(&dest_file)?;
        println!("✓ Removed toolchain eronom-{} ({})", name, dest_file.display());
    } else {
        println!("Toolchain eronom-{} is not installed in ~/.eronom/toolchains/bin/", name);
    }

    Ok(())
}
