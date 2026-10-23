use std::env;
use std::path::Path;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let mir_dir = Path::new(&manifest_dir).join("deps").join("mir");

    println!("cargo:rerun-if-changed={}", mir_dir.join("mir.c").display());
    println!("cargo:rerun-if-changed={}", mir_dir.join("mir-gen.c").display());
    println!("cargo:rerun-if-changed={}", mir_dir.join("mir.h").display());
    println!("cargo:rerun-if-changed={}", mir_dir.join("mir-gen.h").display());

    cc::Build::new()
        .file(mir_dir.join("mir.c"))
        .file(mir_dir.join("mir-gen.c"))
        .include(&mir_dir)
        .flag_if_supported("-Wno-psabi")
        .flag_if_supported("-fno-unwind-tables")
        .flag_if_supported("-fno-asynchronous-unwind-tables")
        .warnings(false)
        .compile("mir");

    let out_dir = env::var("OUT_DIR").unwrap();
    let native_dir = Path::new(&manifest_dir).join("target").join("er_native");
    let _ = std::fs::create_dir_all(&native_dir);

    // Copy compiled static libraries from OUT_DIR to target/er_native/
    if let Ok(entries) = std::fs::read_dir(&out_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if ext == "lib" || ext == "a" {
                    if let Some(fname) = path.file_name() {
                        let _ = std::fs::copy(&path, native_dir.join(fname));
                    }
                }
            }
        }
    }

    // Mirror libraries across standard naming conventions (.lib, lib*.a, lib*.lib)
    for name in &["mir"] {
        let candidates = [
            format!("{}.lib", name),
            format!("lib{}.a", name),
            format!("lib{}.lib", name),
        ];
        for src_name in &candidates {
            let src_file = Path::new(&out_dir).join(src_name);
            if src_file.exists() {
                for dst_name in &candidates {
                    let _ = std::fs::copy(&src_file, native_dir.join(dst_name));
                }
                break;
            }
        }
    }

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
}
