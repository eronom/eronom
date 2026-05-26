use std::env;
use std::path::Path;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let mir_dir = Path::new(&manifest_dir).join("external").join("mir");

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

    // Compile uSockets C files
    let u_sockets_dir = Path::new(&manifest_dir).join("external").join("uWebSockets").join("uSockets");
    let mut u_sockets_build = cc::Build::new();
    u_sockets_build
        .file(u_sockets_dir.join("src").join("bsd.c"))
        .file(u_sockets_dir.join("src").join("context.c"))
        .file(u_sockets_dir.join("src").join("loop.c"))
        .file(u_sockets_dir.join("src").join("socket.c"))
        .file(u_sockets_dir.join("src").join("udp.c"))
        .file(u_sockets_dir.join("src").join("eventing").join("epoll_kqueue.c"))
        .include(u_sockets_dir.join("src"))
        .define("LIBUS_NO_SSL", None)
        .warnings(false)
        .opt_level(3)
        .compile("usockets");

    // Compile uWebSockets C++ wrapper
    let u_websockets_dir = Path::new(&manifest_dir).join("external").join("uWebSockets");
    let mut u_websockets_build = cc::Build::new();
    println!("cargo:rerun-if-changed=src/vm/er_http.cpp");
    u_websockets_build
        .cpp(true)
        .std("c++17")
        .file("src/vm/er_http.cpp")
        .include(u_websockets_dir.join("src"))
        .include(u_sockets_dir.join("src"))
        .define("LIBUS_NO_SSL", None)
        .define("UWS_NO_ZLIB", None)
        .warnings(false)
        .opt_level(3)
        .compile("er_http");
}
