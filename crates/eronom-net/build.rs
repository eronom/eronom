use std::env;
use std::path::Path;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let root_dir = Path::new(&manifest_dir).join("../..");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let is_windows = target_os == "windows";

    let libuv_dir = root_dir.join("deps").join("libuv");
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
    let u_sockets_dir = root_dir.join("deps").join("uWebSockets").join("uSockets");
    let mut u_sockets_build = cc::Build::new();
    u_sockets_build
        .file(u_sockets_dir.join("src").join("bsd.c"))
        .file(u_sockets_dir.join("src").join("context.c"))
        .file(u_sockets_dir.join("src").join("loop.c"))
        .file(u_sockets_dir.join("src").join("socket.c"))
        .file(u_sockets_dir.join("src").join("udp.c"))
        .include(u_sockets_dir.join("src"))
        .define("LIBUS_NO_SSL", None)
        .flag_if_supported("-fno-unwind-tables")
        .flag_if_supported("-fno-asynchronous-unwind-tables")
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
    let u_websockets_dir = root_dir.join("deps").join("uWebSockets");
    let er_http_cpp = root_dir.join("src").join("vm").join("er_http.cpp");
    let er_clif_cpp = root_dir.join("src").join("vm").join("er_clif.cpp");

    println!("cargo:rerun-if-changed={}", er_http_cpp.display());
    println!("cargo:rerun-if-changed={}", er_clif_cpp.display());

    let mut u_websockets_build = cc::Build::new();
    u_websockets_build
        .cpp(true)
        .std("c++17")
        .file(&er_http_cpp)
        .file(&er_clif_cpp)
        .include(u_websockets_dir.join("src"))
        .include(u_sockets_dir.join("src"))
        .define("LIBUS_NO_SSL", None)
        .define("UWS_NO_ZLIB", None)
        .flag_if_supported("-fno-exceptions")
        .flag_if_supported("-fno-rtti")
        .flag_if_supported("-fno-unwind-tables")
        .flag_if_supported("-fno-asynchronous-unwind-tables")
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

    let out_dir = env::var("OUT_DIR").unwrap();
    let native_dir = root_dir.join("target").join("er_native");
    let _ = std::fs::create_dir_all(&native_dir);

    // Copy compiled static libraries to target/er_native/
    for name in &["er_http", "usockets", "uv"] {
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
}
