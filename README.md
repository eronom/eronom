# Eronom ⚡

<p align="center">
  <img src="eronom_logo.png" alt="Eronom Logo" width="180" />
</p>

<p align="center">
  <strong>High-performance, JIT-compiled scripting language runtime, reactive template engine, and web server framework.</strong>
</p>

<p align="center">
  <a href="#-quickstart-in-60-seconds">Quickstart</a> •
  <a href="#-key-features">Key Features</a> •
  <a href="#-syntax-showcase">Syntax Showcase</a> •
  <a href="#-cli-commands">CLI Commands</a> •
  <a href="#-architecture--deep-dive-docs">Documentation</a> •
  <a href="AGENTS.md">AI Agent Guide</a>
</p>

---

## 💡 What is Eronom?

**Eronom** is an all-in-one execution platform engineered for low-latency web services, reactive applications, and standalone native binary compilation:

1. **Custom Virtual Machine & JIT Engine**: Combines a register-based bytecode VM with Vladimir Makarov's lightweight **MIR JIT compiler** and a pure-Rust **Cranelift AOT backend**.
2. **Native C++ Networking with Dynamic HMR**: Powered by native C++ `uWebSockets` bindings. Reloads routing and VM logic on-the-fly *without* terminating active TCP connections or restarting the main OS process.
3. **Colorless Structured Concurrency**: Executes non-blocking I/O and asynchronous tasks directly inside ordinary functions without the viral infectiousness of async/await function coloring.
4. **ERM Reactive Component Engine**: Single-file component framework (`.erm`) combining server-rendered HTML, scoped CSS, and client-side reactive state binding (`useState`).
5. **Standalone Native Machine Binaries**: Direct ahead-of-time (AOT) machine code generation for Linux (ELF), Windows (PE64), and macOS (Mach-O) without requiring an external runtime installation.

---

## ⚡ Quickstart in 60 Seconds

### 1. Prerequisites
Ensure you have Rust (`cargo`), a C/C++ compiler (GCC, Clang, or MSVC), and CMake installed.

### 2. Build Eronom
```bash
# Clone the repository
git clone https://github.com/eronom/eronom.git
cd eronom

# Build debug binary
cargo build
```

### 3. Create & Run Your First HTTP Service
Create `server.er`:

```rust
import { serve, Response } from "std/http"

serve({
    port: 3000,
    fetch: (req) => {
        if (req.path == "/health") {
            return Response.text("OK", { status: 200 })
        }
        return Response.json({
            status: "online",
            path: req.path,
            method: req.method,
            timestamp: "2026-10-20"
        })
    }
})
```

Run the server:
```bash
cargo run -- server.er
```
Visit `http://localhost:3000/health`. Edit `server.er` while the server is running—Eronom automatically swaps in the new logic on the next incoming request!

---

## 🚀 Key Features

* **High-Throughput, Low-Latency Networking**: Integrates kernel-level I/O event demultiplexing (`epoll` on Linux, `kqueue` on macOS/BSD, and `libuv` on Windows) via native C++ `uWebSockets` FFI.
* **Connection-Preserving Hot Reloading (HMR)**: Live-reloads script routes instantly upon file save while keeping open TCP socket connections intact.
* **Zero Async Function Coloring**: Non-blocking I/O without the burden of marking entire call stacks as `async`/`await`.
* **Scoped Reactivity & Server Rendering**: Full support for Server-Side Rendering (SSR), Static Site Generation (SSG), and Partial Prerendering (PPR) using `.erm` component files.
* **Mark-and-Sweep GC with Map Pooling**: Custom Garbage Collector equipped with pre-allocated pooled HashMaps to eliminate heap churn and allocator fragmentation under high HTTP loads.
* **Ahead-of-Time Native Binary Compilation**: Compile `.er` scripts straight into standalone native executables (`eronom build --aot`).

---

## 📝 Syntax Showcase

### 1. Colorless Structured Concurrency
```rust
import { fetch } from "std/http"

const fetchEndpoint = (endpoint) => {
    print("Initiating request to {endpoint}")
    const res = fetch("https://api.example.com/{endpoint}") // Non-blocking without async/await!
    print("Received {endpoint} response")
    return res
}

// Structured Concurrency: Automatically joins both concurrent branches
concurrent {
    fetchEndpoint("metrics")
    fetchEndpoint("health")
}

print("Both tasks completed successfully!")
```

### 2. Struct Composition & Flattened Embedding
Eronom supports explicit nested composition as well as zero-overhead flattened embedding via the `embed` keyword:

```rust
struct Transform {
    x: int,
    y: int,
}

// Embeds Transform: Promotes fields x and y directly into Player
struct Player embed Transform {
    name: string,
    health: int,
}

// Flat initialization
let hero = Player {
    x: 100,
    y: 250,
    name: "Knight",
    health: 100,
}

// Direct field access (single continuous heap layout)
print("Position: {hero.x}, {hero.y} | Hero: {hero.name}")
```

### 3. Reactive Single-File Components (`.erm`)
Single-file component combining server rendering, scoped styling, and client-side reactive state:

```html
<script>
    let count = useState(0)

    const increment = () => {
        count++
    }
</script>

<div class="counter-box">
    <h3>Current Count: {count}</h3>
    <button onClick={increment}>Increment Count</button>
</div>

<style>
    .counter-box {
        padding: 20px;
        background: #18181b;
        color: #fafafa;
        border-radius: 8px;
    }
    button {
        background: #3b82f6;
        color: white;
        border: none;
        padding: 8px 16px;
        border-radius: 4px;
        cursor: pointer;
    }
</style>
```

---

## 🛠️ CLI Commands

The `eronom` CLI provides an integrated suite for full-stack development:

| Command | Description | Example |
| :--- | :--- | :--- |
| **`eronom <script.er>`** | Execute a script directly | `eronom server.er` |
| **`eronom init`** | Scaffold a new Eronom application | `eronom init my-app --git` |
| **`eronom dev`** | Start local dev server with WebSocket HMR | `eronom dev -p 3000` |
| **`eronom build`** | Compile for SSR, SSG, or standalone native binary | `eronom build --aot -o dist/server` |
| **`eronom start`** | Run production server | `eronom start -p 8080` |
| **`eronom check`** | Statically type-check scripts (supports `--json`) | `eronom check server.er --json` |
| **`eronom test`** | Run integrated test suites | `eronom test tests/` |
| **`eronom transpile`**| Transpile `.er` script to JavaScript | `eronom transpile main.er` |
| **`eronom toolchain`**| Manage optional native compilers | `eronom toolchain add cranelift` |

---

## 📁 Repository Directory Map

```text
eronom/
├── src/
│   ├── frontend/         # Lexer, recursive-descent parser, AST, static type checker
│   ├── vm/               # Stack bytecode VM, Garbage Collector, uWebSockets C++ FFI
│   ├── jit/              # Vladimir Makarov's MIR JIT and native AOT compiler (ELF/PE/Mach-O)
│   ├── toolchain/        # Toolchain discovery and Cranelift native backend
│   └── cli/              # CLI argument parser and commands
├── libs/
│   └── erm/              # Reactive component template compiler, SSR/SSG engine, HMR server
├── std/                  # Standard libraries (std/http.er, std/io.er)
├── deps/                 # Vendored native C/C++ dependencies (mir, uWebSockets, libuv)
├── docs/                 # In-depth architectural and developer documentation
├── AGENTS.md             # Developer & AI coding agent architecture manual
└── README.md             # Project landing page and quickstart
```

---

## 📚 Architecture & Deep-Dive Docs

Dive deeper into Eronom's internals and guides:

* 🏗️ **[Architecture & Internals](docs/architecture.md)**: Virtual machine, instruction set, MIR JIT, AOT machine code gen, GC map pooling, and C++ FFI networking.
* 📖 **[Language Tour & Syntax Guide](docs/language-tour.md)**: Comprehensive language specification, types, structs, embedding, and concurrency.
* 🧩 **[ERM Component Framework](docs/erm-framework.md)**: Component syntax, `useState`, template blocks, scoped CSS, and SSR/SSG/PPR.
* ⚙️ **[CLI Reference & Config](docs/cli-reference.md)**: Detailed command options, `eronom.toml` schema, and environment variables.
* 🤝 **[Contributing Guide](docs/contributing.md)**: Development setup, build rules, testing, and Git workflow.
* 🤖 **[AI Agent Guide (AGENTS.md)](AGENTS.md)**: Complete guide and invariants for autonomous AI coding agents.

---

## 📄 License

Eronom is open-source software licensed under the [MIT License](LICENSE).
