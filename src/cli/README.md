# Eronom CLI Subsystem (`src/cli/`)

The CLI subsystem handles command-line argument parsing, subcommand dispatch, project scaffolding, build pipelines, and self-contained binary bundling.

---

## Architecture & Subsystems

```text
src/cli/
├── commands.rs     # Clap argument parser, subcommands, and configuration resolver
├── init.rs         # Project scaffolding and template extractor (init command)
├── build.rs        # Web bundle builder (SSR, SSG, PPR) and native compiler dispatcher
├── bundle.rs       # Self-contained executable bundler (embedded app runtime)
├── spinner.rs      # Interactive terminal progress spinner
└── mod.rs          # CLI runner and command dispatcher
```

---

## Subcommands Overview

### 1. `commands.rs`
- Defines the `Cli` structure and `Commands` enum via `clap`.
- Resolves server listening ports in priority order: CLI flag (`-p`) > positional argument > `PORT` environment variable > `eronom.toml` configuration.
- Parses `eronom.toml` server settings (`[server] port = 3000`).

### 2. `init.rs` (`eronom init`)
- Scaffolds a new project structure with starter templates and dependencies.
- Supports offline initialization (`--offline`), custom templates (`--template`), and Git repo setup (`--git`).

### 3. `build.rs` & `bundle.rs` (`eronom build`)
- Dispatches web build modes:
  - `--ssr`: Dynamic server rendering (default).
  - `--ssg`: Static site pre-rendering.
  - `--ppr`: Partial prerendering with streaming reactive islands.
- Dispatches native machine code compilation:
  - `--aot`: Native machine code via `src/jit/aot/`.
  - `--cranelift`: Pure-Rust Cranelift compiler.
- Packages self-contained single-binary executables via `bundle.rs` by embedding script assets directly into a native runner stub.

### 4. `spinner.rs`
- Provides an animated terminal spinner for long-running operations (such as compilation, toolchain downloads, or template extraction).
