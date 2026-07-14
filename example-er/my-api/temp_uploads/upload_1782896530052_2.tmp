# Multi-Runtime HTTP Server Benchmarks

We have implemented the identical `/todos` HTTP API endpoint across four different runtimes: Eronom, Node.js, Bun, and Deno. This document details the request handling mechanisms, execution instructions, and benchmark performance comparison.

---

## 1. Server Implementations

### A. Eronom (`server.er`)
- **Engine**: Custom Eronom VM with native C++ `uWebSockets` bindings.
- **Port**: `3000`
- **Handling Mechanism**: Registers route callbacks within a compiled Eronom environment. It runs on a native C++ event loop with automatic hot-reloading (detects file changes and re-evaluates the script context dynamically on the next request).
- **Run Command**:
  ```bash
  cargo run --release -- example-er/my-api/server.er
  ```

### B. Node.js (`server-node.js`)
- **Engine**: Node.js core `http` module.
- **Port**: `3001`
- **Handling Mechanism**: Creates a legacy HTTP server instance with a callback stream handler. Response writing and JSON serialization are managed manually via `writeHead` and `end`.
- **Run Command**:
  ```bash
  node example-er/my-api/server-node.js
  ```

### C. Bun (`server-bun.js`)
- **Engine**: Bun's native HTTP engine via `Bun.serve`.
- **Port**: `3002`
- **Handling Mechanism**: Employs a modern fetch-based server returning standard Web `Response.json(...)` objects, optimized natively inside Bun's runtime.
- **Run Command**:
  ```bash
  bun example-er/my-api/server-bun.js
  ```

### D. Deno (`server-deno.js`)
- **Engine**: Deno's native server via `Deno.serve` (backed by Hyper in Rust).
- **Port**: `3003`
- **Handling Mechanism**: A fetch-based API that takes a standard Web `Request` and returns standard Web `Response.json(...)` objects on a highly optimized Rust network layer.
- **Run Command**:
  ```bash
  deno run --allow-net example-er/my-api/server-deno.js
  ```

---

## 2. Performance Benchmark Results

Below is the comparison of the throughput and latency metrics for each runtime under heavy load:

| Server / Runtime | Port | Core HTTP Engine | Req/Sec (Avg) | Latency (Avg) | Total Requests (5s) | Avg Bytes/Sec |
| :--- | :---: | :--- | :---: | :---: | :---: | :---: |
| **Deno** | `3003` | Hyper (Rust) | **29,630.4** | **2.86 ms** | **148k** | **6.22 MB** |
| **Eronom** | `3000` | uWebSockets (C++) | **28,324.8** | **3.06 ms** | **142k** | **5.95 MB** |
| **Bun** | `3002` | Native (C++) | **27,390.4** | **3.18 ms** | **137k** | **5.50 MB** |
| **Node.js** | `3001` | http parser (C++) | **20,372.0** | **4.44 ms** | **102k** | **5.15 MB** |

---

## 3. How to Run the Benchmarks

All benchmarks were executed using `autocannon` with 100 concurrent connections over a duration of 5 seconds:

```bash
npx autocannon -c 100 -d 5 http://localhost:<PORT>/todos
```

### Highlights
- **Deno** and **Eronom** lead the throughput benchmarks with Eronom's C++ uWebSockets bindings matching the performance characteristics of modern JS runtimes.
- **Eronom** performs similarly to Deno and Bun, with the added capability of hot-reloading routes and VM data logic instantly on source file changes without dropping the network socket connection.
