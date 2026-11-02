# Eronom Standard Library (`std/`)

The Eronom standard library provides built-in modules for HTTP networking, file I/O, cryptography, task scheduling, process management, and testing.

---

## Module Catalog

| Module | Import Path | Description | Key Exports |
| :--- | :--- | :--- | :--- |
| **HTTP** | `"std/http"` | High-performance HTTP server & response builder | `serve`, `router`, `Response`, `status`, `fetch` |
| **I/O** | `"std/io"` | File reading, writing, and structured concurrency | `file`, `write`, `Io` |
| **Filesystem** | `"std/fs"` | Low-level filesystem operations | `readFile`, `writeFile`, `exists`, `remove` |
| **Crypto** | `"std/crypto"` | Cryptographic hashing and HMAC algorithms | `sha256`, `hmac`, `randomBytes` |
| **Tasks** | `"std/task"` | Structured concurrency and asynchronous tasks | `spawn`, `join`, `all` |
| **Schedule** | `"std/schedule"` | Recurring timers, intervals, and schedules | `interval`, `timeout`, `cron` |
| **Clock** | `"std/clock"` | Monotonic timestamps and date utilities | `now`, `elapsed`, `sleep` |
| **Path** | `"std/path"` | Cross-platform filesystem path utilities | `join`, `dirname`, `basename`, `extname` |
| **Process** | `"std/process"` | Process management and environment access | `exit`, `pid`, `args` |
| **Environment**| `"std/env"` | System environment variable access | `get`, `set`, `has` |
| **JSON** | `"std/json"` | Native JSON serialization and parsing | `parse`, `stringify` |
| **Testing** | `"std/test"` | Built-in test runner and assertion assertions | `describe`, `it`, `expect`, `assert` |
| **ERM** | `"std/erm"` | Low-level reactivity primitives | `init`, `get`, `set`, `update` |

---

## Code Examples

### 1. HTTP Server (`std/http`)
```rust
import { serve, Response, status } from "std/http"

serve({
    port: 3000,
    fetch: (req) => {
        if (req.path == "/health") {
            return Response.text("OK", { status: status.OK })
        }
        return Response.json({ status: "healthy", path: req.path })
    }
})
```

### 2. File & Stream I/O (`std/io`)
```rust
import { file, write } from "std/io"

let logFile = file("app.log")
if (logFile.exists()) {
    print("Log content:\n{logFile.text()}")
}

write("output.txt", "Eronom runtime output\n")
```

### 3. Concurrency & Tasks (`std/task`)
```rust
import { spawn } from "std/task"

let task1 = spawn(() => { return 10 * 2 })
let task2 = spawn(() => { return 20 * 2 })

print("Result: {task1.wait() + task2.wait()}")
```
