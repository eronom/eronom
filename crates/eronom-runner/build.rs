use std::env;
use std::path::Path;

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "linux" {
        println!("cargo:rustc-link-arg=-no-pie");
        println!("cargo:rustc-link-arg=-Wl,-z,lazy");
        println!("cargo:rustc-link-arg=-Wl,--hash-style=gnu");

        if has_static_libstdcxx() {
            println!("cargo:rustc-link-arg=-static-libstdc++");
            println!("cargo:rustc-link-arg=-static-libgcc");
        }
    }
}

fn has_static_libstdcxx() -> bool {
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
