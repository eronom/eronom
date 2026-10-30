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
}
