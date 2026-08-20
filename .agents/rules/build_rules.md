---
trigger: always_on
---

# Eronom Build, CLI, and Binary Deployment Rules

Follow these mandatory rules when building, compiling, running, or deploying the Eronom runtime and applications:

## 1. Build Performance
- **Prefer Fast Debug Builds**: Always run `cargo build` instead of `cargo build --release`. 
- **Release Build Warning**: Eronom release builds require heavy optimization steps for native uWebSockets C++ code and MIR JIT compilation, making `cargo build --release` time-consuming.

## 2. Running & Executing Scripts
- **Execute Raw Eronom Scripts**: Use `cargo run -- path/to/script.er` to test scripts without installing the CLI globally.
- **Run Development Server**: Test HTTP API and HMR features using:
  ```bash
  cargo run -- example-er/my-api/server.er
  ```
  Or navigate to the project directory and run:
  ```bash
  eronom dev
  ```

## 3. Project Creation
- **CLI Project Initialization**: Use `eronom init` (or `eronom init <dir>`) to initialize a fresh Eronom application structure.

## 4. Mandatory Post-Rust Modification Binary Deployment Rule
- **Rebuild & Deploy Rule**: Whenever any Rust source file (`.rs`) in `src/` or `libs/` is modified, you MUST perform the following post-change workflow:
  1. Run `cargo build` to compile the debug binary.
  2. Copy the resulting `eronom` binary from `target/debug/eronom` to:
     - The `eronom` folder in the user's home path (`~/eronom/` / `/home/vishnus/eronom/`)
     - The `example-er/` folder in the workspace root (`example-er/`)

```bash
cargo build && cp target/debug/eronom ~/eronom/ && cp target/debug/eronom example-er/
```
