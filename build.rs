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
        .warnings(false)
        .compile("mir");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let is_windows = target_os == "windows";

    let libuv_dir = Path::new(&manifest_dir).join("deps").join("libuv");
    if is_windows {
        let mut libuv_build = cc::Build::new();
        let libuv_src = libuv_dir.join("src");
        let libuv_win = libuv_src.join("win");

        libuv_build
            .include(libuv_dir.join("include"))
            .include(&libuv_src)
            .include(&libuv_win)
            .define("_WIN32_WINNT", "0x0A00")
            .define("WIN32_LEAN_AND_MEAN", None)
            .define("_CRT_DECLARE_NONSTDC_NAMES", "0")
            .define("_CRT_SECURE_NO_WARNINGS", None)
            .warnings(false)
            .opt_level(3)
            // Common files
            .file(libuv_src.join("fs-poll.c"))
            .file(libuv_src.join("idna.c"))
            .file(libuv_src.join("inet.c"))
            .file(libuv_src.join("random.c"))
            .file(libuv_src.join("strscpy.c"))
            .file(libuv_src.join("strtok.c"))
            .file(libuv_src.join("thread-common.c"))
            .file(libuv_src.join("threadpool.c"))
            .file(libuv_src.join("timer.c"))
            .file(libuv_src.join("uv-common.c"))
            .file(libuv_src.join("uv-data-getter-setters.c"))
            .file(libuv_src.join("version.c"))
            // Windows files
            .file(libuv_win.join("async.c"))
            .file(libuv_win.join("core.c"))
            .file(libuv_win.join("detect-wakeup.c"))
            .file(libuv_win.join("dl.c"))
            .file(libuv_win.join("error.c"))
            .file(libuv_win.join("fs.c"))
            .file(libuv_win.join("fs-event.c"))
            .file(libuv_win.join("getaddrinfo.c"))
            .file(libuv_win.join("getnameinfo.c"))
            .file(libuv_win.join("handle.c"))
            .file(libuv_win.join("loop-watcher.c"))
            .file(libuv_win.join("pipe.c"))
            .file(libuv_win.join("thread.c"))
            .file(libuv_win.join("poll.c"))
            .file(libuv_win.join("process.c"))
            .file(libuv_win.join("process-stdio.c"))
            .file(libuv_win.join("signal.c"))
            .file(libuv_win.join("snprintf.c"))
            .file(libuv_win.join("stream.c"))
            .file(libuv_win.join("tcp.c"))
            .file(libuv_win.join("tty.c"))
            .file(libuv_win.join("udp.c"))
            .file(libuv_win.join("util.c"))
            .file(libuv_win.join("winapi.c"))
            .file(libuv_win.join("winsock.c"))
            .compile("uv");
    }

    // Compile uSockets C files
    let u_sockets_dir = Path::new(&manifest_dir).join("deps").join("uWebSockets").join("uSockets");
    let mut u_sockets_build = cc::Build::new();
    u_sockets_build
        .file(u_sockets_dir.join("src").join("bsd.c"))
        .file(u_sockets_dir.join("src").join("context.c"))
        .file(u_sockets_dir.join("src").join("loop.c"))
        .file(u_sockets_dir.join("src").join("socket.c"))
        .file(u_sockets_dir.join("src").join("udp.c"))
        .include(u_sockets_dir.join("src"))
        .define("LIBUS_NO_SSL", None)
        .warnings(false)
        .opt_level(3);

    if is_windows {
        u_sockets_build.file(u_sockets_dir.join("src").join("eventing").join("libuv.c"));
        u_sockets_build.include(libuv_dir.join("include"));
        u_sockets_build.define("LIBUS_USE_LIBUV", None);
        u_sockets_build.define("WIN32_LEAN_AND_MEAN", None);
        u_sockets_build.define("_CRT_SECURE_NO_WARNINGS", None);
    } else {
        u_sockets_build.file(u_sockets_dir.join("src").join("eventing").join("epoll_kqueue.c"));
    }

    u_sockets_build.compile("usockets");

    // Compile uWebSockets C++ wrapper
    let u_websockets_dir = Path::new(&manifest_dir).join("deps").join("uWebSockets");
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
        .opt_level(3);

    if is_windows {
        u_websockets_build.define("LIBUS_USE_LIBUV", None);
        u_websockets_build.include(libuv_dir.join("include"));
        u_websockets_build.define("WIN32_LEAN_AND_MEAN", None);
        u_websockets_build.define("NOMINMAX", None);
        u_websockets_build.define("_CRT_SECURE_NO_WARNINGS", None);
        println!("cargo:rustc-link-lib=psapi");
        println!("cargo:rustc-link-lib=user32");
        println!("cargo:rustc-link-lib=advapi32");
        println!("cargo:rustc-link-lib=iphlpapi");
        println!("cargo:rustc-link-lib=userenv");
        println!("cargo:rustc-link-lib=ws2_32");
        println!("cargo:rustc-link-lib=dbghelp");
        println!("cargo:rustc-link-lib=ole32");
        println!("cargo:rustc-link-lib=shell32");
    }

    u_websockets_build.compile("er_http");
}
