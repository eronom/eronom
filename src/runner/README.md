# Eronom Script Runner & Runtime Orchestrator (`src/runner/`)

The runner subsystem manages the execution lifecycle of Eronom scripts, embedded standalone bundles, native bindings, and test suites.

---

## Architecture & Subsystems

```text
src/runner/
├── file_runner.rs      # Primary execution harness for .er script files
├── test_runner.rs      # Eronom test discovery and execution engine
├── bundle.rs           # Embedded standalone binary bundle extractor and loader
├── natives.rs          # Registration of native standard library functions in the VM
├── http_detect.rs      # HTTP server startup detection and socket event loop hooks
├── embedded_bundle.rs  # Standalone execution harness for embedded bundles
└── mod.rs              # Runner module exports
```

---

## Key Responsibilities

1. **Script Execution (`file_runner.rs`)**:
   - Reads, parses, compiles, and evaluates `.er` scripts within the VM environment.
   - Coordinates with `http_detect.rs` to keep the native C++ event loop alive if an HTTP server (`serve`) is declared.

2. **Self-Contained Embedded Bundles (`bundle.rs`)**:
   - Detects whether the current executable contains an embedded script payload appended during `eronom build`.
   - Bypasses external file reads and directly executes the embedded application in standalone deployments.

3. **Native Function Registration (`natives.rs`)**:
   - Exposes platform primitives (filesystem, console output, crypto, timer, networking) into the VM global scope and standard library modules (`std/io`, `std/http`).

4. **Test Harness (`test_runner.rs`)**:
   - Discovers and runs tests via `eronom test [file]`.
   - Aggregates test assertions and outputs status reports.
