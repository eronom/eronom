use std::path::Path;
use std::process::Command;
use crate::frontend::parse_and_resolve_imports;
use crate::vm::compiler::Compiler;
use super::lower::lower_program;
use super::generator::generate_native_c;

fn find_zig() -> Option<std::path::PathBuf> {
    if let Ok(output) = Command::new("zig").arg("version").output() {
        if output.status.success() {
            return Some(std::path::PathBuf::from("zig"));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let p = std::path::PathBuf::from(home).join(".zvm/bin/zig");
        if p.exists() {
            return Some(p);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let p = std::path::PathBuf::from(home).join(".eronom/toolchain/bin/zig");
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn get_aot_cache_dir() -> Option<std::path::PathBuf> {
    if std::env::var("ERONOM_NO_CACHE").map(|v| v == "1" || v == "true").unwrap_or(false)
        || std::env::var("ERONOM_AOT_NO_CACHE").map(|v| v == "1" || v == "true").unwrap_or(false)
    {
        return None;
    }
    if let Ok(dir) = std::env::var("ERONOM_CACHE_DIR") {
        return Some(std::path::PathBuf::from(dir).join("aot"));
    }
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        return Some(std::path::PathBuf::from(xdg).join("eronom").join("aot"));
    }
    if let Ok(home) = std::env::var("HOME") {
        return Some(std::path::PathBuf::from(home).join(".cache").join("eronom").join("aot"));
    }
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        return Some(std::path::PathBuf::from(userprofile).join(".cache").join("eronom").join("aot"));
    }
    None
}

fn resolve_target_triple(target: &str) -> (&'static str, &'static str) {
    let lower = target.to_lowercase();
    if lower == "windows"
        || lower == "win"
        || lower == "win64"
        || lower == "x86_64-windows"
        || lower.contains("windows-gnu")
        || lower.contains("windows-msvc")
    {
        ("x86_64-windows", "windows")
    } else if lower == "windows-arm64" || lower == "win-arm64" || lower == "aarch64-windows" {
        ("aarch64-windows", "windows")
    } else if lower == "macos"
        || lower == "darwin"
        || lower == "mac"
        || lower == "apple"
        || lower == "macos-arm64"
        || lower == "aarch64-macos"
        || lower == "aarch64-apple-darwin"
    {
        ("aarch64-macos", "macos")
    } else if lower == "macos-x86_64"
        || lower == "macos-intel"
        || lower == "x86_64-macos"
        || lower == "x86_64-apple-darwin"
    {
        ("x86_64-macos", "macos")
    } else if lower == "linux"
        || lower == "linux-x86_64"
        || lower == "x86_64-linux"
        || lower.contains("linux-gnu")
    {
        ("x86_64-linux", "linux")
    } else if lower == "linux-arm64" || lower == "linux-aarch64" || lower == "aarch64-linux" {
        ("aarch64-linux", "linux")
    } else if lower.contains("musl") {
        ("x86_64-linux-musl", "linux")
    } else {
        ("custom", "unknown")
    }
}

pub fn resolve_runner_stub(custom_stub: Option<&Path>) -> Option<std::path::PathBuf> {
    if let Some(s) = custom_stub {
        if s.exists() {
            return Some(s.to_path_buf());
        }
    }

    // 1. Direct micro-runner candidates relative to CWD
    for candidate in &[
        "stubs/eronom-runner",
        "stubs/er_runner",
        "target/release/eronom-runner",
        "target/release/er_runner",
        "target/debug/eronom-runner",
        "target/debug/er_runner",
        "stubs/eronom-runner.exe",
        "stubs/er_runner.exe",
    ] {
        let p = std::path::PathBuf::from(candidate);
        if p.exists() {
            return Some(p);
        }
    }

    // 2. Relative to current_exe and its ancestors
    if let Ok(p) = std::env::current_exe() {
        let canonical = p.canonicalize().unwrap_or_else(|_| p.clone());
        let name = canonical.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if (name == "eronom-runner" || name == "eronom-runner.exe" || name == "er_runner" || name == "er_runner.exe") && canonical.exists() {
            return Some(canonical);
        }

        let mut curr = canonical.parent();
        while let Some(parent) = curr {
            let direct0 = parent.join("eronom-runner");
            if direct0.exists() {
                return Some(direct0);
            }
            let direct1 = parent.join("er_runner");
            if direct1.exists() {
                return Some(direct1);
            }
            let stub0 = parent.join("stubs/eronom-runner");
            if stub0.exists() {
                return Some(stub0);
            }
            let stub1 = parent.join("stubs/er_runner");
            if stub1.exists() {
                return Some(stub1);
            }
            let tr0 = parent.join("target/release/eronom-runner");
            if tr0.exists() {
                return Some(tr0);
            }
            let td0 = parent.join("target/debug/eronom-runner");
            if td0.exists() {
                return Some(td0);
            }
            curr = parent.parent();
        }
    }

    // 3. Walk ancestors of current_dir
    if let Ok(cwd) = std::env::current_dir() {
        let mut curr = Some(cwd.as_path());
        while let Some(parent) = curr {
            let direct0 = parent.join("eronom-runner");
            if direct0.exists() {
                return Some(direct0);
            }
            let direct1 = parent.join("er_runner");
            if direct1.exists() {
                return Some(direct1);
            }
            let stub0 = parent.join("stubs/eronom-runner");
            if stub0.exists() {
                return Some(stub0);
            }
            let stub1 = parent.join("stubs/er_runner");
            if stub1.exists() {
                return Some(stub1);
            }
            let tr0 = parent.join("target/release/eronom-runner");
            if tr0.exists() {
                return Some(tr0);
            }
            let td0 = parent.join("target/debug/eronom-runner");
            if td0.exists() {
                return Some(td0);
            }
            curr = parent.parent();
        }
    }

    // 4. HOME directory
    if let Ok(home) = std::env::var("HOME") {
        let home_stub1 = std::path::PathBuf::from(&home).join(".eronom/stubs/eronom-runner");
        if home_stub1.exists() {
            return Some(home_stub1);
        }
        let home_stub = std::path::PathBuf::from(&home).join(".eronom/stubs/er_runner");
        if home_stub.exists() {
            return Some(home_stub);
        }
        let home_bin1 = std::path::PathBuf::from(&home).join("eronom/eronom-runner");
        if home_bin1.exists() {
            return Some(home_bin1);
        }
    }

    // 5. Last resort fallback to full CLI only if no micro-runner exists
    for candidate in &[
        "target/release/eronom",
        "target/debug/eronom",
    ] {
        let p = std::path::PathBuf::from(candidate);
        if p.exists() {
            return Some(p);
        }
    }

    if let Ok(p) = std::env::current_exe() {
        let canonical = p.canonicalize().unwrap_or_else(|_| p.clone());
        if let Some(parent) = canonical.parent() {
            let candidate = parent.join("eronom");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    None
}

fn find_target_candidate_in_hierarchy(candidates: &[&str]) -> Option<std::path::PathBuf> {
    for cand in candidates {
        let p = std::path::PathBuf::from(cand);
        if p.exists() {
            return Some(p);
        }
    }

    if let Ok(p) = std::env::current_exe() {
        let canonical = p.canonicalize().unwrap_or_else(|_| p.clone());
        let mut curr = canonical.parent();
        while let Some(parent) = curr {
            for cand in candidates {
                let candidate_path = parent.join(cand);
                if candidate_path.exists() {
                    return Some(candidate_path);
                }
            }
            curr = parent.parent();
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        let mut curr = Some(cwd.as_path());
        while let Some(parent) = curr {
            for cand in candidates {
                let candidate_path = parent.join(cand);
                if candidate_path.exists() {
                    return Some(candidate_path);
                }
            }
            curr = parent.parent();
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        let home_dir = std::path::PathBuf::from(&home);
        for cand in candidates {
            let p1 = home_dir.join(".eronom").join(cand);
            if p1.exists() {
                return Some(p1);
            }
            let p2 = home_dir.join("eronom").join(cand);
            if p2.exists() {
                return Some(p2);
            }
        }
    }

    None
}

pub fn resolve_target_stub(custom_stub: Option<&Path>, target: Option<&str>) -> Option<std::path::PathBuf> {
    if let Some(s) = custom_stub {
        if s.exists() {
            return Some(s.to_path_buf());
        }
    }
    if let Some(target_str) = target {
        let (_, os_kind) = resolve_target_triple(target_str);
        if os_kind == "windows" {
            return find_target_candidate_in_hierarchy(&[
                "stubs/eronom-runner-win-x64.exe",
                "stubs/eronom-runner.exe",
                "stubs/windows/eronom-runner.exe",
                "stubs/er_runner.exe",
                "stubs/windows/er_runner.exe",
            ]);
        } else if os_kind == "macos" {
            return find_target_candidate_in_hierarchy(&[
                "stubs/eronom-runner-mac-arm64",
                "stubs/eronom-runner-mac",
                "stubs/macos/eronom-runner",
                "stubs/er_runner_mac",
                "stubs/macos/er_runner",
            ]);
        } else if os_kind == "linux" {
            if let Some(stub) = find_target_candidate_in_hierarchy(&[
                "stubs/eronom-runner",
                "stubs/eronom-runner-linux",
                "stubs/linux/eronom-runner",
                "stubs/er_runner",
                "stubs/linux/er_runner",
            ]) {
                return Some(stub);
            }
        }
    }
    resolve_runner_stub(custom_stub)
}

/// Build an Eronom script ahead-of-time directly into a standalone native binary.
pub fn build_aot_binary(script_path: &Path, output_path: &Path) -> anyhow::Result<()> {
    build_aot_binary_full(script_path, output_path, None, false, None)
}

/// Build an Eronom script ahead-of-time with optional cross-compilation target.
pub fn build_aot_binary_with_target(
    script_path: &Path,
    output_path: &Path,
    target: Option<&str>,
) -> anyhow::Result<()> {
    build_aot_binary_full(script_path, output_path, target, false, None)
}

/// Build an Eronom script ahead-of-time directly in-process without invoking any external C compiler.
pub fn build_aot_binary_in_process(
    script_path: &Path,
    output_path: &Path,
) -> anyhow::Result<()> {
    build_aot_binary_full(script_path, output_path, None, true, None)
}

/// Build an Eronom script ahead-of-time with optional target, in-process preference, and custom runner stub.
pub fn build_aot_binary_with_target_and_options(
    script_path: &Path,
    output_path: &Path,
    target: Option<&str>,
    prefer_in_process: bool,
    custom_stub: Option<&Path>,
) -> anyhow::Result<()> {
    build_aot_binary_full(script_path, output_path, target, prefer_in_process, custom_stub)
}

fn build_aot_binary_full(
    script_path: &Path,
    output_path: &Path,
    target: Option<&str>,
    prefer_in_process: bool,
    custom_stub: Option<&Path>,
) -> anyhow::Result<()> {
    if !script_path.exists() {
        anyhow::bail!("Source script not found: {}", script_path.display());
    }

    // 1. Parse and resolve module imports
    let stmts = match parse_and_resolve_imports(script_path) {
        Ok(s) => s,
        Err(e) => anyhow::bail!("Compile/Import error in {}: {}", script_path.display(), e),
    };

    // 2. Compile AST statements into bytecode function
    let compiler = Compiler::new();
    let function = match compiler.compile(&stmts) {
        Ok(f) => f,
        Err(e) => anyhow::bail!("Bytecode lowering error: {}", e),
    };

    // 3. Lower bytecode function to typed E-IR module
    let module_name = script_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "er_module".to_string());
    let eir_module = lower_program(&function, &module_name);

    // 4. Generate native C / MIR machine code representation
    let c_source = generate_native_c(&eir_module);

    // 5. Ensure parent output directory exists and handle platform extensions
    let mut final_out = output_path.to_path_buf();
    if let Some(target_str) = target {
        let (_, os_kind) = resolve_target_triple(target_str);
        if os_kind == "windows" {
            let file_name = final_out
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if !file_name.ends_with(".exe") {
                final_out = final_out.with_file_name(format!("{}.exe", file_name));
            }
        }
    }

    if let Some(parent) = final_out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    let env_in_process = std::env::var("ERONOM_AOT_IN_PROCESS")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false);
    let use_in_process = prefer_in_process || env_in_process;

    // Content-addressed AOT binary caching
    let cache_dir = get_aot_cache_dir();
    let cache_file = if let Some(ref cdir) = cache_dir {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(c_source.as_bytes());
        hasher.update(target.unwrap_or("host").as_bytes());
        hasher.update((if use_in_process { "in_process" } else { "cc" }).as_bytes());
        let hash_hex = format!("{:x}", hasher.finalize());
        let _ = std::fs::create_dir_all(cdir);
        Some(cdir.join(hash_hex))
    } else {
        None
    };

    if let Some(ref cfile) = cache_file {
        if cfile.exists() && cfile.metadata().map(|m| m.len() > 0).unwrap_or(false) {
            if std::fs::copy(cfile, &final_out).is_ok() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&final_out, std::fs::Permissions::from_mode(0o755));
                }
                return Ok(());
            }
        }
    }

    if use_in_process {
        let _is_cross = if let Some(target_str) = target {
            let (_, os_kind) = resolve_target_triple(target_str);
            #[cfg(target_os = "linux")]
            let host_os = "linux";
            #[cfg(target_os = "windows")]
            let host_os = "windows";
            #[cfg(target_os = "macos")]
            let host_os = "macos";
            os_kind != host_os
        } else {
            false
        };

        // 1. If custom runner stub is provided, always use it
        if let Some(custom) = custom_stub {
            if custom.exists() {
                crate::cli::bundle::create_standalone_binary(custom, script_path, None, &final_out)?;
                return Ok(());
            }
        }

        // 2. Try Cranelift backend first for pure-Rust native code generation
        let no_clif = std::env::var("ERONOM_NO_CRANELIFT").map(|v| v == "1").unwrap_or(false);
        if !no_clif {
            if super::cranelift::build_cranelift_binary(script_path, &final_out, target).is_ok() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&final_out, std::fs::Permissions::from_mode(0o755));
                }
                if let Some(ref cfile) = cache_file {
                    let _ = std::fs::copy(&final_out, cfile);
                }
                return Ok(());
            }
        }

        // 3. Fallback: If a runner stub is available and not in pure-native mode, use it to ensure runnability
        let pure_native = std::env::var("ERONOM_AOT_PURE_NATIVE").map(|v| v == "1").unwrap_or(false);
        if !pure_native {
            if let Some(stub_path) = resolve_target_stub(custom_stub, target) {
                crate::cli::bundle::create_standalone_binary(&stub_path, script_path, None, &final_out)?;
                return Ok(());
            }
        }

        // 3. Otherwise (cross-platform with zero external toolchains or pure native mode), build in-process via MIR + object crate
        let target_platform = if let Some(target_str) = target {
            super::object_builder::TargetPlatform::from_str(target_str)
        } else {
            super::object_builder::TargetPlatform::host()
        };

        let bytes = super::object_builder::build_aot_native_executable(&eir_module, target_platform)?;
        std::fs::write(&final_out, &bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&final_out, std::fs::Permissions::from_mode(0o755));
        }
        if let Some(ref cfile) = cache_file {
            let _ = std::fs::copy(&final_out, cfile);
        }
        return Ok(());
    }

    // 6. Write temporary native source and invoke platform/target linker
    let temp_dir = std::env::temp_dir();
    let temp_c = temp_dir.join(format!("er_aot_{}_{}.c", module_name, std::process::id()));
    std::fs::write(&temp_c, &c_source)?;

    let status = if let Some(target_str) = target {
        let (zig_target, _) = resolve_target_triple(target_str);
        let target_triple = if zig_target == "custom" { target_str } else { zig_target };

        if let Some(zig) = find_zig() {
            Command::new(zig)
                .arg("cc")
                .arg("-O3")
                .arg("-fomit-frame-pointer")
                .arg("-s")
                .arg("-target")
                .arg(target_triple)
                .arg(&temp_c)
                .arg("-o")
                .arg(&final_out)
                .status()?
        } else {
            #[cfg(target_os = "linux")]
            let is_host = target_str == "linux" || target_str.contains("linux");
            #[cfg(target_os = "macos")]
            let is_host = target_str == "macos" || target_str.contains("darwin");
            #[cfg(target_os = "windows")]
            let is_host = target_str == "windows";

            if is_host && Command::new("cc").arg("--version").output().is_ok() {
                Command::new("cc")
                    .arg("-O3")
                    .arg("-fomit-frame-pointer")
                    .arg("-fno-math-errno")
                    .arg(&temp_c)
                    .arg("-o")
                    .arg(&final_out)
                    .status()?
            } else if is_host {
                if let Some(stub_path) = resolve_runner_stub(custom_stub) {
                    let _ = std::fs::remove_file(&temp_c);
                    crate::cli::bundle::create_standalone_binary(&stub_path, script_path, None, &final_out)?;
                    return Ok(());
                } else {
                    let _ = std::fs::remove_file(&temp_c);
                    anyhow::bail!("No native C compiler found and runner stub unavailable.");
                }
            } else {
                let _ = std::fs::remove_file(&temp_c);
                anyhow::bail!(
                    "Cross-compilation for target '{}' requires 'zig' to be installed on your system.\nPlease install zig or ensure it is in PATH.",
                    target_str
                );
            }
        }
    } else {
        // Native build: prefer host cc, fallback to zig cc, or zero-dependency in-process builder
        let has_cc = Command::new("cc")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        if has_cc {
            Command::new("cc")
                .arg("-O3")
                .arg("-fomit-frame-pointer")
                .arg("-fno-math-errno")
                .arg(&temp_c)
                .arg("-o")
                .arg(&final_out)
                .status()?
        } else if find_zig().is_some() {
            let zig = find_zig().unwrap();
            Command::new(zig)
                .arg("cc")
                .arg("-O3")
                .arg("-fomit-frame-pointer")
                .arg("-s")
                .arg(&temp_c)
                .arg("-o")
                .arg(&final_out)
                .status()?
        } else if let Some(stub_path) = resolve_runner_stub(custom_stub) {
            let _ = std::fs::remove_file(&temp_c);
            crate::cli::bundle::create_standalone_binary(&stub_path, script_path, None, &final_out)?;
            return Ok(());
        } else {
            let _ = std::fs::remove_file(&temp_c);
            anyhow::bail!("No native C compiler found. Please install 'cc' (gcc/clang) or 'zig'.");
        }
    };

    let _ = std::fs::remove_file(&temp_c);

    if !status.success() {
        anyhow::bail!("Native compilation failed with exit code: {:?}", status.code());
    }

    if let Some(ref cfile) = cache_file {
        let _ = std::fs::copy(&final_out, cfile);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_aot_binary_end_to_end() {
        let temp_dir = std::env::temp_dir();
        let script_path = temp_dir.join(format!("test_script_{}.er", std::process::id()));
        let bin_path = temp_dir.join(format!("test_bin_{}", std::process::id()));

        // Simple Eronom test script
        std::fs::write(&script_path, "let a = 10\nlet b = 20\nlet c = a + b\n").unwrap();

        let res = build_aot_binary(&script_path, &bin_path);
        let _ = std::fs::remove_file(&script_path);

        assert!(res.is_ok(), "build_aot_binary should succeed: {:?}", res.err());
        assert!(bin_path.exists(), "Binary file should exist on disk");

        // Execute the compiled native binary
        let output = Command::new(&bin_path).output().expect("Failed to execute binary");
        let _ = std::fs::remove_file(&bin_path);

        assert!(output.status.success(), "Compiled binary exited with status {:?}", output.status);
    }

    #[test]
    fn test_build_aot_closure_and_functions() {
        let temp_dir = std::env::temp_dir();
        let script_path = temp_dir.join(format!("test_closure_{}.er", std::process::id()));
        let bin_path = temp_dir.join(format!("test_closure_bin_{}", std::process::id()));

        let script = r#"
let num = 5;
names = "Eronom"
function test() {
    print(num)
}
test()
"#;
        std::fs::write(&script_path, script).unwrap();
        let res = build_aot_binary(&script_path, &bin_path);
        let _ = std::fs::remove_file(&script_path);

        assert!(res.is_ok(), "build_aot_binary failed: {:?}", res.err());
        let output = Command::new(&bin_path).output().expect("Failed to execute binary");
        let _ = std::fs::remove_file(&bin_path);

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(stdout.trim(), "5");
    }

    #[test]
    fn test_build_aot_objects_and_arrays() {
        let temp_dir = std::env::temp_dir();
        let script_path = temp_dir.join(format!("test_obj_arr_{}.er", std::process::id()));
        let bin_path = temp_dir.join(format!("test_obj_arr_bin_{}", std::process::id()));

        let script = r#"
let obj = { x: 10, y: 20 };
print(obj.x);
print(obj.y);
let arr = [100, 200];
print(arr[0]);
print(arr[1]);
"#;
        std::fs::write(&script_path, script).unwrap();
        let res = build_aot_binary(&script_path, &bin_path);
        let _ = std::fs::remove_file(&script_path);

        assert!(res.is_ok(), "build_aot_binary failed: {:?}", res.err());
        let output = Command::new(&bin_path).output().expect("Failed to execute binary");
        let _ = std::fs::remove_file(&bin_path);

        assert!(
            output.status.success(),
            "status: {:?}, stdout: {}, stderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.trim().lines().collect();
        assert_eq!(lines, vec!["10", "20", "100", "200"]);
    }
    #[test]
    fn test_build_aot_fibonacci() {
        let temp_dir = std::env::temp_dir();
        let bin_path = temp_dir.join(format!("test_fib_bin_{}", std::process::id()));
        let res = build_aot_binary(Path::new("fib.er"), &bin_path);
        assert!(res.is_ok(), "build_aot_binary failed: {:?}", res.err());
        let output = Command::new(&bin_path).output().expect("Failed to execute binary");
        let _ = std::fs::remove_file(&bin_path);

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(stdout.trim(), "317811");
    }

    #[test]
    fn test_build_aot_in_process_without_cc() {
        let temp_dir = std::env::temp_dir();
        let script_path = temp_dir.join(format!("test_in_process_{}.er", std::process::id()));
        let bin_path = temp_dir.join(format!("test_in_process_bin_{}", std::process::id()));

        std::fs::write(&script_path, "let a = 15;\nlet b = 25;\nprint(a + b);\n").unwrap();

        let res = build_aot_binary_in_process(&script_path, &bin_path);
        let _ = std::fs::remove_file(&script_path);

        assert!(res.is_ok(), "build_aot_binary in-process failed: {:?}", res.err());
        assert!(bin_path.exists(), "Binary file should exist on disk");

        let output = Command::new(&bin_path).output().expect("Failed to execute in-process binary");
        let _ = std::fs::remove_file(&bin_path);

        assert!(output.status.success(), "In-process binary exited with status {:?}", output.status);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(stdout.trim(), "40");
    }

    #[test]
    fn test_cross_platform_windows_in_process() {
        let temp_dir = std::env::temp_dir();
        let script_path = temp_dir.join(format!("test_win_{}.er", std::process::id()));
        let bin_path = temp_dir.join(format!("test_win_app_{}.exe", std::process::id()));

        std::fs::write(&script_path, "let x = 100;\nlet y = 200;\nprint(x + y);\n").unwrap();

        let res = build_aot_binary_with_target_and_options(
            &script_path,
            &bin_path,
            Some("windows"),
            true, // prefer_in_process
            None,
        );
        let _ = std::fs::remove_file(&script_path);

        assert!(res.is_ok(), "Windows in-process cross compilation failed: {:?}", res.err());
        assert!(bin_path.exists(), "Windows binary should exist");

        let metadata = std::fs::metadata(&bin_path).unwrap();
        // Verify standalone binary size is within micro-runner budget (< 5 MB)
        assert!(metadata.len() > 1000 && metadata.len() < 5_000_000, "Binary size should be within runner budget, got {}", metadata.len());

        let _ = std::fs::remove_file(&bin_path);
    }

    #[test]
    fn test_cross_platform_macos_in_process() {
        let temp_dir = std::env::temp_dir();
        let script_path = temp_dir.join(format!("test_mac_{}.er", std::process::id()));
        let bin_path = temp_dir.join(format!("test_mac_app_{}", std::process::id()));

        std::fs::write(&script_path, "let x = 10;\nlet y = 20;\nprint(x * y);\n").unwrap();

        let res = build_aot_binary_with_target_and_options(
            &script_path,
            &bin_path,
            Some("macos"),
            true, // prefer_in_process
            None,
        );
        let _ = std::fs::remove_file(&script_path);

        assert!(res.is_ok(), "macOS in-process cross compilation failed: {:?}", res.err());
        assert!(bin_path.exists(), "macOS binary should exist");

        let metadata = std::fs::metadata(&bin_path).unwrap();
        // Verify standalone binary size is within micro-runner budget (< 5 MB)
        assert!(metadata.len() > 1000 && metadata.len() < 5_000_000, "Binary size should be within runner budget, got {}", metadata.len());

        let _ = std::fs::remove_file(&bin_path);
    }
}

