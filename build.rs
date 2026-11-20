use std::env;
use std::path::{Path, PathBuf};

fn collect_er_files(dir: &Path, base: &Path, out: &mut Vec<(String, PathBuf)>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut sorted: Vec<_> = entries.flatten().collect();
        sorted.sort_by_key(|e| e.path());
        for entry in sorted {
            let path = entry.path();
            if path.is_dir() {
                collect_er_files(&path, base, out);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("er") {
                if let Ok(rel) = path.strip_prefix(base) {
                    let module_key = format!("std/{}", rel.to_string_lossy().replace('\\', "/"));
                    out.push((module_key, path));
                }
            }
        }
    }
}

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = env::var("OUT_DIR").unwrap();

    // Auto-scan std/ directory recursively and generate embedded standard library table for standalone bundles
    let std_dir = Path::new(&manifest_dir).join("std");
    println!("cargo:rerun-if-changed={}", std_dir.display());

    let mut code = String::from("pub const EMBEDDED_STD_MODULES: &[(&str, &[u8])] = &[\n");
    let mut files = Vec::new();
    collect_er_files(&std_dir, &std_dir, &mut files);
    files.sort_by(|a, b| a.0.cmp(&b.0));

    for (module_key, path) in files {
        println!("cargo:rerun-if-changed={}", path.display());
        let rel_from_manifest = path.strip_prefix(&manifest_dir).unwrap_or(&path);
        let rel_str = rel_from_manifest.to_string_lossy().replace('\\', "/");
        code.push_str(&format!(
            "    (\"{}\", include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/{}\"))),\n",
            module_key, rel_str
        ));
    }
    code.push_str("];\n");
    let embedded_std_path = Path::new(&out_dir).join("embedded_std.rs");
    std::fs::write(&embedded_std_path, code).unwrap();
}
