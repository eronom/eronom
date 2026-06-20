# Eronom Codebase Architecture Guide

AI agents working on this project should orient themselves using the following directory layout:

## 1. Core Executable (Rust & C++)
The main execution pipeline is divided into:
* **`src/main.rs`**: Entrypoint of the runtime. Handles command-line arguments and script startup.
* **`src/frontend/`**: The language frontend.
  * `lexer.rs`: Lexer (tokenization).
  * `parser.rs`: Parser (generating the AST).
  * `ast.rs`: AST nodes.
* **`src/vm/`**: Bytecode compiler, stack VM, garbage collector, and native FFI.
  * `compiler.rs`: Compiles Eronom AST to custom bytecode.
  * `execute.rs`: Bytecode interpreter loop.
  * `value.rs`: Runtime representation of Eronom values (string, object, array, numbers).
  * `gc.rs`: Mark-and-sweep Garbage Collector for heap values.
  * `er_http.rs` & `er_http.cpp`: high-performance HTTP server wrapper bridging uWebSockets.
* **`src/jit/`**: Just-In-Time Compiler.
  * `compiler.rs` & `bindings.rs`: Generates MIR intermediate representation from Eronom VM bytecode.

## 2. Standard Libraries & CLI (Rust & Eronom)
* **`libs/erm/`**: Rust library code for compiling templates, handling CLI subcommands, and managing server requests.
  * `cli.rs`: Command parsing via `clap`.
  * `compiler.rs`: Compiles Eronom template (`.erm`) files to HTML pages.
  * `server.rs`: Dev and production server logic.
* **`std/`**: Standard libraries written in Eronom itself.
  * `http.er`: Contains the standard HTTP Router/Server constructs.
  * `io.er`: File and system I/O methods.
* **`libs/init/`**: Default project template files copied by the `eronom init` command.
  * `core/runtime.js`: Reactivity runtime for template bindings and loops.
  * `core/hmr.js`: Hot Module Reload (HMR) websocket client.
  * `app/pages/` & `app/layouts/`: Base files for web apps.
