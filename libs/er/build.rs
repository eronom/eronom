use std::env;
use std::path::Path;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let mir_dir = Path::new(&manifest_dir).join("../../mir");

    println!("cargo:rerun-if-changed={}", mir_dir.join("mir.c").display());
    println!("cargo:rerun-if-changed={}", mir_dir.join("mir-gen.c").display());
    println!("cargo:rerun-if-changed={}", mir_dir.join("mir.h").display());
    println!("cargo:rerun-if-changed={}", mir_dir.join("mir-gen.h").display());

    cc::Build::new()
        .file(mir_dir.join("mir.c"))
        .file(mir_dir.join("mir-gen.c"))
        .include(&mir_dir)
        .warnings(false)
        .compile("mir");
}
