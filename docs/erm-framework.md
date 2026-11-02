# ERM Component Framework & Reactive Engine

ERM (Eronom Reactive Modules) is the native full-stack template and component framework built into the Eronom runtime. It compiles single-file `.erm` templates into high-performance server-rendered HTML with lightweight client-side reactivity and scoped styling.

---

## 1. Single-File Component Structure (`.erm`)

An `.erm` file brings together logic, markup, and styling into a clean, unified component:

```html
<script>
    let count = useState(0)
    let title = "Interactive Counter"

    const increment = () => {
        count++
    }

    const decrement = () => {
        count--
    }
</script>

<div class="counter-card">
    <h2>{title}</h2>
    <p class="display">Current Count: <strong>{count}</strong></p>

    <div class="button-group">
        <button onClick={decrement}>- Decrement</button>
        <button onClick={increment}>+ Increment</button>
    </div>
</div>

<style>
    .counter-card {
        padding: 24px;
        border-radius: 12px;
        background: #18181b;
        color: #f4f4f5;
        border: 1px solid #27272a;
    }
    .display {
        font-size: 1.25rem;
        margin: 16px 0;
    }
    button {
        background: #3b82f6;
        color: white;
        border: none;
        border-radius: 6px;
        padding: 8px 16px;
        cursor: pointer;
        font-weight: 500;
        margin-right: 8px;
    }
    button:hover {
        background: #2563eb;
    }
</style>
```

---

## 2. Client-Side Reactivity Model

### `useState` Primitive
State declared with `useState` registers reactive dependencies within the component tree:
- State updates (`count++` or assignments) compile to micro-runtime primitives (`erm.get`, `erm.set`, `erm.update`).
- When state mutates, the runtime performs surgical DOM updates targeting only affected data attributes (`data-erm-*`), eliminating full-page re-renders.

### Function-Based Page Components
Pages can also declare parameters and routing data directly via exported functions:

```html
<script>
    import Header from './components/Header.erm'

    export fn page(params) {
        let username = useState(params.user || "Guest")
        
        <main>
            <Header title="User Dashboard" />
            <h1>Welcome, {username}!</h1>
        </main>
    }
</script>
```

---

## 3. Template Directives

### Conditionals (`{#if}`)
Conditionally render DOM subtrees using `{#if}` and `{:else}` blocks:

```html
{#if user.isLoggedIn}
    <p>Welcome back, {user.name}!</p>
    <button onClick={logout}>Sign Out</button>
{:else}
    <p>Please log in to continue.</p>
    <Link to="/login">Sign In</Link>
{/#if}
```

### Iteration (`{#for}`)
Render dynamic collections with `{#for}` blocks:

```html
<ul class="task-list">
    {#for task in tasks}
        <li class="task-item">
            <span>{task.title}</span>
            <span class="badge">{task.status}</span>
        </li>
    {/#for}
</ul>
```

### Routing Links (`<Link>`)
Client-side transitions are enabled using the built-in `<Link>` component:

```html
<Link to="/about">About Us</Link>
```

---

## 4. Scoped Styling

Styles declared inside the `<style>` block are scoped automatically by the ERM compiler:
- CSS class selectors are analyzed and rewritten with unique component attributes (e.g. `[data-erm-scope]`).
- Styles remain isolated to the declaring component without leaking into global styles or parent components.

---

## 5. Rendering Modes

Eronom supports three distinct compilation and rendering modes configured at build time:

### A. Server-Side Rendering (SSR) — Default
```bash
eronom build --ssr
```
- Pages are dynamically compiled and rendered on each incoming HTTP request.
- Ideal for real-time dashboards, authenticated pages, and dynamic data APIs.

### B. Static Site Generation (SSG)
```bash
eronom build --ssg
```
- Pre-renders all routes into pure static HTML, CSS, and JS assets at compile time.
- Ideal for documentation, marketing pages, and content sites served from edge storage.

### C. Partial Prerendering (PPR)
```bash
eronom build --ppr
```
- Generates a static HTML shell for instantaneous initial page response, while streaming in dynamic reactive islands asynchronously.

---

## 6. Live Hot Module Reloading (HMR)

During local development with `eronom dev`:
1. The ERM development server serves your application and establishes an internal WebSocket connection with the browser client.
2. When you modify an `.erm` component, the compiler re-parses only the changed file.
3. The server broadcasts a targeted diff over the WebSocket.
4. The client replaces the updated DOM nodes in-place while preserving current component state!
