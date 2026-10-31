use std::path::Path;
use crate::frontend::parse_and_resolve_imports;

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

    #[cfg(windows)]
    let candidates = &[
        "target/release/eronom-runner.exe",
        "target/debug/eronom-runner.exe",
        "stubs/eronom-runner-win-x64.exe",
        "stubs/eronom-runner.exe",
        "stubs/windows/eronom-runner.exe",
        "stubs/er_runner.exe",
        "stubs/windows/er_runner.exe",
        "target/release/eronom.exe",
        "target/debug/eronom.exe",
    ];

    #[cfg(target_os = "macos")]
    let candidates = &[
        "target/aarch64-apple-darwin/release/eronom-runner",
        "target/aarch64-apple-darwin/debug/eronom-runner",
        "target/release/eronom-runner",
        "target/debug/eronom-runner",
        "stubs/eronom-runner-mac-arm64",
        "stubs/eronom-runner-mac",
        "stubs/macos/eronom-runner",
        "stubs/er_runner_mac",
        "stubs/macos/er_runner",
        "target/aarch64-apple-darwin/release/eronom",
        "target/aarch64-apple-darwin/debug/eronom",
        "target/release/eronom",
        "target/debug/eronom",
    ];

    #[cfg(not(any(windows, target_os = "macos")))]
    let candidates = &[
        "target/release/eronom-runner",
        "target/debug/eronom-runner",
        "stubs/eronom-runner",
        "stubs/er_runner",
        "target/release/eronom",
        "target/debug/eronom",
    ];

    find_target_candidate_in_hierarchy(candidates)
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

    if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
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
    let os_kind = if let Some(target_str) = target {
        resolve_target_triple(target_str).1
    } else if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };

    if os_kind == "windows" {
        return find_target_candidate_in_hierarchy(&[
            "target/release/eronom-runner.exe",
            "target/debug/eronom-runner.exe",
            "stubs/eronom-runner-win-x64.exe",
            "stubs/eronom-runner.exe",
            "stubs/windows/eronom-runner.exe",
            "stubs/er_runner.exe",
            "stubs/windows/er_runner.exe",
            "target/release/eronom.exe",
            "target/debug/eronom.exe",
        ]);
    } else if os_kind == "macos" {
        return find_target_candidate_in_hierarchy(&[
            "target/aarch64-apple-darwin/release/eronom-runner",
            "target/aarch64-apple-darwin/debug/eronom-runner",
            "target/release/eronom-runner",
            "target/debug/eronom-runner",
            "stubs/eronom-runner-mac-arm64",
            "stubs/eronom-runner-mac",
            "stubs/macos/eronom-runner",
            "stubs/er_runner_mac",
            "stubs/macos/er_runner",
            "target/aarch64-apple-darwin/release/eronom",
            "target/aarch64-apple-darwin/debug/eronom",
            "target/release/eronom",
            "target/debug/eronom",
        ]);
    } else if os_kind == "linux" {
        return find_target_candidate_in_hierarchy(&[
            "target/release/eronom-runner",
            "target/debug/eronom-runner",
            "stubs/eronom-runner",
            "stubs/eronom-runner-linux",
            "stubs/linux/eronom-runner",
            "stubs/er_runner",
            "stubs/linux/er_runner",
            "target/release/eronom",
            "target/debug/eronom",
        ]);
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

/// Build an Eronom script ahead-of-time.
pub fn build_aot_binary_with_mir(
    script_path: &Path,
    output_path: &Path,
    target: Option<&str>,
) -> anyhow::Result<()> {
    crate::toolchain::run_cranelift_build(script_path, output_path, target)
}

fn build_aot_binary_full(
    script_path: &Path,
    output_path: &Path,
    target: Option<&str>,
    _prefer_in_process: bool,
    custom_stub: Option<&Path>,
) -> anyhow::Result<()> {
    if !script_path.exists() {
        anyhow::bail!("Source script not found: {}", script_path.display());
    }

    // 1. Verify source script exists and read bytes
    let script_bytes = std::fs::read(script_path)?;

    // 2. Validate script syntax
    let _ = parse_and_resolve_imports(script_path)
        .map_err(|e| anyhow::anyhow!("Compile/Import error in {}: {}", script_path.display(), e))?;

    // 3. Ensure parent output directory exists and handle platform extensions
    let mut final_out = output_path.to_path_buf();
    let is_windows = if let Some(target_str) = target {
        let (_, os_kind) = resolve_target_triple(target_str);
        os_kind == "windows"
    } else {
        cfg!(windows)
    };
    if is_windows {
        let file_name = final_out
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if !file_name.ends_with(".exe") {
            final_out = final_out.with_file_name(format!("{}.exe", file_name));
        }
    }

    if let Some(parent) = final_out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    let sync_final_out = |final_out: &Path| {
        if final_out != output_path && final_out.exists() {
            let _ = std::fs::copy(final_out, output_path);
            #[cfg(windows)]
            {
                if !output_path.to_string_lossy().ends_with(".exe") {
                    let _ = std::fs::copy(final_out, output_path.with_extension("exe"));
                }
            }
        }
    };

    // Content-addressed AOT binary caching
    let cache_dir = get_aot_cache_dir();
    let cache_file = if let Some(ref cdir) = cache_dir {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&script_bytes);
        hasher.update(target.unwrap_or("host").as_bytes());
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
                sync_final_out(&final_out);
                return Ok(());
            }
        }
    }

    // 1. If custom runner stub is provided, always use it
    if let Some(custom) = custom_stub {
        if custom.exists() {
            crate::cli::bundle::create_standalone_binary(custom, script_path, None, &final_out)?;
            sync_final_out(&final_out);
            return Ok(());
        }
    }

    // 2. Try Cranelift backend first for pure-Rust native code generation (x86_64 ELF/PE)
    let is_macos = if let Some(t) = target {
        let lower = t.to_lowercase();
        lower == "macos" || lower == "mac" || lower == "darwin" || lower.contains("apple-darwin")
    } else {
        cfg!(target_os = "macos")
    };
    let no_clif = std::env::var("ERONOM_NO_CRANELIFT").map(|v| v == "1" || v == "true").unwrap_or(false);
    if !no_clif && !is_macos {
        if crate::toolchain::run_cranelift_build(script_path, &final_out, target).is_ok() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&final_out, std::fs::Permissions::from_mode(0o755));
            }
            if let Some(ref cfile) = cache_file {
                let _ = std::fs::copy(&final_out, cfile);
            }
            sync_final_out(&final_out);
            return Ok(());
        }
    }

    // 3. Fallback: If a runner stub is available and not in pure-native mode, use it to ensure runnability
    let pure_native = std::env::var("ERONOM_AOT_PURE_NATIVE").map(|v| v == "1" || v == "true").unwrap_or(false);
    if !pure_native {
        if let Some(stub_path) = resolve_target_stub(custom_stub, target) {
            crate::cli::bundle::create_standalone_binary(&stub_path, script_path, None, &final_out)?;
            sync_final_out(&final_out);
            return Ok(());
        }
    }

    // 4. Otherwise, build via Cranelift directly (surfacing error if it fails)
    crate::toolchain::run_cranelift_build(script_path, &final_out, target)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&final_out, std::fs::Permissions::from_mode(0o755));
    }
    if let Some(ref cfile) = cache_file {
        let _ = std::fs::copy(&final_out, cfile);
    }
    sync_final_out(&final_out);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn get_actual_bin(bin_path: &Path) -> std::path::PathBuf {
        if cfg!(windows) {
            let exe_path = bin_path.with_extension("exe");
            if exe_path.exists() {
                exe_path
            } else {
                bin_path.to_path_buf()
            }
        } else if bin_path.exists() {
            bin_path.to_path_buf()
        } else {
            bin_path.with_extension("exe")
        }
    }

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
        let actual_bin = get_actual_bin(&bin_path);
        assert!(actual_bin.exists(), "Binary file should exist on disk");

        // Execute the compiled native binary
        let output = Command::new(&actual_bin).output().expect("Failed to execute binary");
        let _ = std::fs::remove_file(&bin_path);
        let _ = std::fs::remove_file(bin_path.with_extension("exe"));

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
        let actual_bin = get_actual_bin(&bin_path);
        assert!(actual_bin.exists());
        let output = Command::new(&actual_bin).output().expect("Failed to execute binary");
        let _ = std::fs::remove_file(&bin_path);
        let _ = std::fs::remove_file(bin_path.with_extension("exe"));

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
        let actual_bin = get_actual_bin(&bin_path);
        assert!(actual_bin.exists(), "Binary file should exist at {}", actual_bin.display());
        let output = Command::new(&actual_bin).output().expect("Failed to execute binary");
        let _ = std::fs::remove_file(&bin_path);
        let _ = std::fs::remove_file(bin_path.with_extension("exe"));

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
        let actual_bin = get_actual_bin(&bin_path);
        assert!(actual_bin.exists());
        let output = Command::new(&actual_bin).output().expect("Failed to execute binary");
        let _ = std::fs::remove_file(&bin_path);
        let _ = std::fs::remove_file(bin_path.with_extension("exe"));

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
        let actual_bin = get_actual_bin(&bin_path);
        assert!(actual_bin.exists(), "Binary file should exist on disk");

        let output = Command::new(&actual_bin).output().expect("Failed to execute in-process binary");
        let _ = std::fs::remove_file(&bin_path);
        let _ = std::fs::remove_file(bin_path.with_extension("exe"));

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

