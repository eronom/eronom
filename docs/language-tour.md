# Eronom Language Tour & Syntax Specification

Eronom is a statically typed, high-performance scripting language with an expressive, modern syntax designed for high-throughput backend services and web applications.

---

## 1. Variables and Constants

Variables are declared using `let` or `const`. Types can be explicitly annotated or inferred automatically:

```rust
// Inferred types
let score = 100
const maxRetries = 3

// Explicit type annotations
let username: string = "developer"
let isActive: bool = true
let latencyMs: float = 4.25

// Constants cannot be reassigned
const API_VERSION = "1.0.0"
```

### Supported Core Types
- **`int`**: 64-bit signed integer.
- **`float`**: 64-bit IEEE floating-point number.
- **`bool`**: Boolean value (`true` or `false`).
- **`string`**: UTF-8 string with built-in interpolation.
- **`array`**: Dynamically sized indexed collection (`[1, 2, 3]`).
- **`object`**: Key-value dictionary (`{ name: "Eronom", port: 3000 }`).
- **`struct`**: User-defined schema-backed data structure.
- **`null`**: Absence of value.

---

## 2. String Interpolation

Expressions inside `{}` are evaluated and coerced to strings directly inside double-quoted strings:

```rust
let service = "Eronom Server"
let port = 3000

print("{service} is listening on http://localhost:{port}")
```

---

## 3. Control Flow

### Conditionals
```rust
let status = 200

if (status >= 200 && status < 300) {
    print("Request succeeded")
} else if (status >= 400 && status < 500) {
    print("Client error")
} else {
    print("Server or unknown status")
}
```

### Loops
Range loops and collection iterators provide clean syntax:

```rust
// Range iteration
let total = 0
for i in 1..100 {
    total = total + i
}

// Array iteration
let items = ["apple", "banana", "cherry"]
for item in items {
    print("Fruit: {item}")
}

// While loops
let counter = 5
while (counter > 0) {
    print("T-minus {counter}")
    counter = counter - 1
}
```

---

## 4. Functions & Closures

Functions in Eronom support default returns, parameters, closures, and arrow expressions:

```rust
// Standard function declaration
fn calculateTax(amount: float, rate: float) {
    return amount * rate
}

// Arrow function / closure
const formatCurrency = (amount) => {
    return "${amount}"
}

// Passing functions as first-class values
let numbers = [1, 2, 3, 4]
let doubled = numbers.map((n) => n * 2)
```

---

## 5. Structs: Composition & Embedding

Eronom provides two complementary ways to structure data: explicit composition and zero-overhead flattened embedding.

### A. Explicit Struct Composition ("Has-A")
Used when an object contains a sub-component as a distinct named field:

```rust
struct Vector2 {
    x: int,
    y: int,
}

struct Entity {
    id: string,
    pos: Vector2, // Explicit nested field
}

// Nested initialization
let hero = Entity {
    id: "hero_01",
    pos: { x: 10, y: 25 },
}

// Explicit traversal
print("Hero position: {hero.pos.x}, {hero.pos.y}")
```

### B. Flattened Struct Embedding (`embed` Keyword)
Used when fields and methods of a parent struct should be promoted directly into the child struct without nested field access:

```rust
struct Position {
    x: int,
    y: int,
}

struct Player embed Position {
    name: string,
    health: int,
}

// Flat initialization: parent fields supplied at top level
let player = Player {
    x: 100,
    y: 200,
    name: "Hero",
    health: 100,
}

// Direct field access (no .pos indirection)
print(player.x) // 100
print(player.name) // "Hero"
```

**Key Advantages of Embedding**:
- **Zero Indirection**: Promotes fields and methods directly onto the child instance.
- **Cache Locality**: Stored in a single, continuous heap allocation rather than two separate heap pointers.
- **Method Overriding**: If a child defines a method identical to the parent, the parent implementation can be invoked via `super.method()`.

---

## 6. Colorless Structured Concurrency

Eronom eliminates the "function color problem" by executing asynchronous operations without requiring viral `async` or `await` modifiers on every calling function.

### Colorless Non-Blocking I/O
I/O primitives automatically yield to the runtime event loop when waiting on network or disk, resuming transparently upon completion:

```rust
import { fetch } from "std/http"

// Ordinary function performing non-blocking network calls
fn fetchProfile(userId) {
    let res = fetch("https://api.example.com/users/{userId}")
    return res.json()
}
```

### Structured Concurrency Scope (`concurrent`)
Spawn concurrent tasks safely with automatic boundary joining:

```rust
import { fetch } from "std/http"

const fetchEndpoint = (endpoint) => {
    print("Starting request to {endpoint}")
    let res = fetch("https://api.example.com/{endpoint}")
    print("Received {endpoint} response")
    return res
}

// Both requests run concurrently and join before exiting the block
concurrent {
    fetchEndpoint("metrics")
    fetchEndpoint("health")
}

print("Both tasks completed!")
```

---

## 7. Standard Libraries

### High-Level HTTP Server (`std/http`)
```rust
import { serve, Response } from "std/http"

serve({
    port: 3000,
    fetch: (req) => {
        if (req.path == "/health") {
            return Response.text("OK", { status: 200 })
        }
        
        return Response.json({
            message: "Hello from Eronom!",
            path: req.path,
            method: req.method,
        })
    }
})
```

### File and Stream I/O (`std/io`)
```rust
import { file, write } from "std/io"

// Read a file
let configFile = file("config.json")
if (configFile.exists()) {
    let configData = configFile.json()
    print("Loaded config: {configData.app_name}")
}

// Write a file
write("output.txt", "Log entry from Eronom runtime\n")
```
