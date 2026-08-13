# Eronom Build and Execution Rules

Follow these rules when building, running, or compiling the Eronom project:

1. **Prefer Fast Builds**: Always run `cargo build` instead of `cargo build --release`. Eronom release builds are extremely time-consuming due to external C++ and JIT compilation dependencies.
2. **Executing Scripts**: Execute test Eronom scripts via the cargo run command:
   ```bash
   cargo run -- path/to/script.er
   ```
3. **Running the Web Server**: To test HTTP and HMR features, run:
   ```bash
   cargo run -- example-er/my-api/server.er
   ```
   Or use the compiled/cli command `eronom dev` in the appropriate directory.
4. **Project Creation**: Use `eronom create <dir>` to initialize a fresh Eronom project (`eronom init` is deprecated and replaced by `create`).
