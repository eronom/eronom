# Eronom CLI Reference & Configuration

The `eronom` CLI provides commands for project initialization, development server orchestration, native ahead-of-time compilation, type checking, and toolchain management.

---

## 1. Direct Script & Binary Execution

### Running Eronom Scripts (`.er`)
Execute any Eronom script directly through the runtime engine:

```bash
eronom server.er
# Or via Cargo development runner:
cargo run -- server.er
```

### Running Standalone Executables (`.exe`)
On Linux x86_64 or Windows, run compiled standalone executables directly:

```bash
eronom dist/app.exe
```

---

## 2. Command Reference

### `eronom init`
Initializes a new Eronom application structure with starter templates and dependencies.

```bash
eronom init [directory] [options]
```

**Options**:
- `[dir]`: Destination directory (defaults to current directory `.`).
- `--template, -t <template>`: Template name or repository to initialize from.
- `--branch, -b <branch>`: Specific branch when using a custom template.
- `--force`: Create the project even if the target directory is not empty.
- `--git`: Automatically initialize a new Git repository.
- `--no-commit`: Prevent creating an initial Git commit when `--git` is set.
- `--offline`: Initialize from bundled local starter files without remote network access.

---

### `eronom dev`
Starts the local development server with instant Hot Module Reloading (HMR) and connection-preserving route recompilation.

```bash
eronom dev [dir_or_port] [-p, --port <number>]
```

**Examples**:
```bash
# Start dev server in current directory on configured port
eronom dev

# Start dev server on port 8080
eronom dev -p 8080

# Start dev server pointing to a specific app directory
eronom dev app/ -p 4000
```

---

### `eronom build`
Compiles Eronom applications into production web bundles or standalone native machine binaries.

```bash
eronom build [dir] [options]
```

**Target & Output Options**:
- `--target, -t <platform>`: Target architecture/OS: `host`, `linux`, `windows`, `macos`.
- `--output, -o <path>`: Custom output file path (e.g. `dist/my-server`).
- `--runner-stub <path>`: Path to a custom runner stub binary for cross-compilation.

**Web Rendering Options**:
- `--ssr`: Build for Server-Side Rendering (default).
- `--ssg`: Build for Static Site Generation (pre-renders pages to static HTML/CSS/JS).
- `--ppr`: Build for Partial Prerendering (static shell with dynamic reactive streaming).

**Native Standalone Compilation Options**:
- `--aot`: Compile script ahead-of-time directly into a standalone native binary without needing an external runtime.
- `--in-process` (or `--no-cc`): Generate native binaries entirely in-process without requiring an external GCC, Clang, or LLVM installation.
- `--cranelift`: Compile using the pure-Rust Cranelift native code generation backend.

---

### `eronom start`
Starts an Eronom application in optimized production mode.

```bash
eronom start [dir_or_port] [-p, --port <number>]
```

---

### `eronom test`
Runs the integrated Eronom test suite.

```bash
# Run all tests
eronom test

# Run a specific test script
eronom test tests/api_test.er
```

---

### `eronom check`
Statically type-checks an Eronom script or project without executing it.

```bash
eronom check <file.er> [--json]
```

**Options**:
- `--json`: Outputs diagnostic reports in machine-readable JSON format for IDEs and autonomous AI agents:

```json
{
  "status": "error",
  "file": "server.er",
  "errors": [
    {
      "code": "E0012",
      "message": "Type mismatch: expected int, found string",
      "loc": { "line": 14, "col": 12 },
      "fixes": [ ... ]
    }
  ]
}
```

---

### `eronom transpile`
Transpiles an Eronom (`.er`) script to standard JavaScript (`.js`).

```bash
eronom transpile <file.er> [-o <output.js>] [--check]
```

---

### `eronom toolchain`
Manages optional compilers and backends (such as Cranelift).

```bash
# List installed and available toolchains
eronom toolchain list

# Install an optional toolchain
eronom toolchain add cranelift

# Remove a toolchain
eronom toolchain remove cranelift
```

---

## 3. Configuration File (`eronom.toml`)

Eronom projects can be customized via a root `eronom.toml` file:

```toml
[server]
port = 3000
host = "0.0.0.0"

[build]
mode = "ssr" # "ssr" | "ssg" | "ppr"
output_dir = "dist"

[toolchain]
backend = "mir" # "mir" | "cranelift"
```

---

## 4. Environment Variables

- **`PORT`**: Overrides the server listening port (takes precedence over `eronom.toml`).
- **`ERONOM_ENV`**: Set to `development` or `production`.
- **`PATH`**: Used by `eronom toolchain` to discover installed native toolchain binaries.
