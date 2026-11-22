use std::env;
use std::path::Path;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = env::var("OUT_DIR").unwrap();

    // Auto-scan std/ directory and generate embedded standard library table for standalone bundles
    let std_dir = Path::new(&manifest_dir).join("std");
    println!("cargo:rerun-if-changed={}", std_dir.display());

    let mut code = String::from("pub const EMBEDDED_STD_MODULES: &[(&str, &[u8])] = &[\n");
    if let Ok(entries) = std::fs::read_dir(&std_dir) {
        let mut files: Vec<_> = entries
            .flatten()
            .filter(|e| e.path().extension().and_then(|ext| ext.to_str()) == Some("er"))
            .collect();
        files.sort_by_key(|e| e.file_name());

        for entry in files {
            let fname = entry.file_name().to_string_lossy().to_string();
            println!("cargo:rerun-if-changed={}", entry.path().display());
            code.push_str(&format!(
                "    (\"std/{}\", include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/std/{}\"))),\n",
                fname, fname
            ));
        }
    }
    code.push_str("];\n");
    let embedded_std_path = Path::new(&out_dir).join("embedded_std.rs");
    std::fs::write(&embedded_std_path, code).unwrap();

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "linux" {
        println!("cargo:rustc-link-arg=-no-pie");
        println!("cargo:rustc-link-arg=-Wl,-z,lazy");
        println!("cargo:rustc-link-arg=-Wl,--hash-style=gnu");

        if has_static_libstdcxx() {
            println!("cargo:rustc-link-arg=-static-libstdc++");
            println!("cargo:rustc-link-arg=-static-libgcc");
        } else {
            println!("cargo:warning=libstdc++.a not detected on host; linking libstdc++.so dynamically. For fully static C++ and zero-relocation releases, install libstdc++-static (Fedora/RHEL).");
        }
    }
}

fn has_static_libstdcxx() -> bool {
    // 1. Query host C++ compiler for the resolved path to libstdc++.a
    for cc in &["g++", "clang++", "c++"] {
        if let Ok(output) = std::process::Command::new(cc)
            .args(["-print-file-name=libstdc++.a"])
            .output()
        {
            if output.status.success() {
                let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if path_str != "libstdc++.a" {
                    let path = Path::new(&path_str);
                    if path.is_file() && path.exists() {
                        return true;
                    }
                }
            }
        }
    }

    // 2. Secondary check across standard system library directories
    for dir in &[
        "/usr/lib/gcc/x86_64-linux-gnu",
        "/usr/lib/gcc/x86_64-redhat-linux",
        "/usr/lib64",
        "/usr/lib",
    ] {
        let base = Path::new(dir);
        if base.exists() {
            if let Ok(entries) = std::fs::read_dir(base) {
                for entry in entries.flatten() {
                    let candidate = entry.path().join("libstdc++.a");
                    if candidate.is_file() && candidate.exists() {
                        return true;
                    }
                }
            }
        }
    }

    false
}
