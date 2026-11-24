use std::env;
use std::path::Path;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let root_dir = Path::new(&manifest_dir).join("../..");
    let mimalloc_dir = root_dir.join("deps").join("mimalloc");

    if mimalloc_dir.exists() {
        let mut build = cc::Build::new();
        build
            .include(mimalloc_dir.join("include"))
            .include(mimalloc_dir.join("src"))
            .flag_if_supported("-fvisibility=hidden")
            .flag_if_supported("-Wno-error=date-time")
            .define("NDEBUG", None)
            .warnings(false)
            .opt_level(3);

        let static_source = mimalloc_dir.join("src").join("static.c");
        println!("cargo:rerun-if-changed={}", static_source.display());
        println!("cargo:rerun-if-changed={}", mimalloc_dir.join("include").join("mimalloc.h").display());

        build.file(static_source);
        build.compile("mimalloc");
    }
}
