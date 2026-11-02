# ERM Reactive Framework (`libs/erm/`)

ERM (Eronom Reactive Modules) is the native full-stack template and component framework built directly into Eronom. It powers reactive single-file `.erm` components, scoped CSS styling, dynamic server-side rendering (SSR/SSG/PPR), and live WebSocket Hot Module Reloading (HMR).

---

## Architecture & Subsystems

```text
libs/erm/
├── compiler/           # Single-pass .erm template compiler
│   ├── blocks.rs       # Control blocks: {#if}, {#for}
│   ├── components.rs   # Nested component composition and props passing
│   ├── css.rs          # Scoped CSS attribute isolation
│   ├── eds.rs          # Element data schema and DOM hydration
│   ├── page.rs         # Layouts, routes, and function-based templates
│   ├── reactivity.rs   # Reactivity lowering: useState -> erm.get / erm.set
│   ├── template_js.rs  # Client-side micro-runtime injection
│   └── tree.rs         # Component AST tree processing
├── server/             # Full-stack HTTP & WebSocket server engine
│   ├── dev.rs          # Development server, file watcher, and HMR engine
│   ├── handler.rs      # HTTP request dispatching and static asset pipeline
│   ├── render.rs       # SSR, SSG, and PPR HTML rendering pipelines
│   ├── router.rs       # File-system-based and programmatic routing tree
│   ├── ffi.rs          # Native server FFI bindings
│   └── types.rs        # Server and request types
├── eval.rs             # Runtime template expression evaluator
├── router.rs           # Client/server route abstractions
└── mod.rs              # ERM module exports
```

---

## Key Capabilities

### 1. Template Compiler (`compiler/`)
- Compiles single-file `.erm` components into hydrated HTML markup, scoped CSS rules, and surgical client-side reactive state handlers.
- Lowers state mutations (`count++`, `value = x`) into runtime reactivity primitives (`erm.init`, `erm.get`, `erm.set`, `erm.update`).
- Encapsulates component styles via unique scoping attributes to prevent global CSS collisions.

### 2. Multi-Mode Rendering Engine (`server/render.rs`)
- **SSR (Server-Side Rendering)**: Dynamically renders pages on incoming requests with live server data.
- **SSG (Static Site Generation)**: Pre-renders pages at build time into standalone static HTML and CSS.
- **PPR (Partial Prerendering)**: Emits static layout shells immediately and streams dynamic reactive blocks asynchronously.

### 3. Live WebSocket HMR (`server/dev.rs`)
- Embedded in the local development server (`eronom dev`).
- Watches `.erm` files for changes, re-compiles modified components, and pushes targeted DOM updates to the browser over an open WebSocket connection without full-page reloads.

---

## Testing

Run ERM compiler and rendering test suites:

```bash
cargo test erm
```
