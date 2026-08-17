use std::env;
use std::path::Path;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let is_windows = target_os == "windows";

    let mir_dir = Path::new(&manifest_dir).join("ext").join("mir");

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

    let wepoll_dir = Path::new(&manifest_dir).join("ext").join("wepoll");

    // Compile uSockets C files
    let u_sockets_dir = Path::new(&manifest_dir).join("ext").join("uWebSockets").join("uSockets");
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
        .define("LIBUS_USE_EPOLL", None)
        .warnings(false)
        .opt_level(3);

    if is_windows {
        u_sockets_build
            .file(wepoll_dir.join("wepoll.c"))
            .include(&wepoll_dir);
    }

    u_sockets_build.compile("usockets");

    // Compile uWebSockets C++ wrapper
    let u_websockets_dir = Path::new(&manifest_dir).join("ext").join("uWebSockets");
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
        .define("LIBUS_USE_EPOLL", None)
        .warnings(false)
        .opt_level(3);

    if is_windows {
        u_websockets_build.include(&wepoll_dir);
    }

    u_websockets_build.compile("er_http");

    if is_windows {
        println!("cargo:rustc-link-lib=ws2_32");
        println!("cargo:rustc-link-lib=userenv");
        println!("cargo:rustc-link-lib=iphlpapi");
        println!("cargo:rustc-link-lib=psapi");
        println!("cargo:rustc-link-lib=shell32");
        println!("cargo:rustc-link-lib=ole32");
        println!("cargo:rustc-link-lib=advapi32");
    }
}
