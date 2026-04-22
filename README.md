# Eronom Dev Server ⚡

An ultra-fast, zero-dependency, Vite-style Hot Module Replacement (HMR) development server built natively in Go. 

Eronom is specifically designed to parse and seamlessly hot-swap `.erm` single-file components (which combine HTML, CSS, and JS) in real-time, completely eliminating the need for full page refreshes during UI development.

## 🌟 Features
* **Zero Dependencies:** No Node.js, `npm`, or massive `node_modules` folders required.
* **Authentic HMR:** Intelligently hot-swaps CSS and surgically replaces inner DOM nodes instantly without blinking the browser.
* **Intelligent Vanilla JS Engine:** Automatically intercepts `DOMContentLoaded` and `load` events, securely dismounts ghost `setInterval` loops, and natively evaluates injected JavaScript without leaking state.
* **Sub-millisecond Feedback:** Powered by Go's native HTTP execution and local filesystem watchers.

## 🚀 Getting Started

### 1. Build the engine
To build the server into a tiny, portable executable, run:
```bash
go build -o eronom-server main.go
```

### 2. Run the dev server
Run the engine in any project directory containing your `.erm` components:
```bash
./eronom-server
```
*(Alternatively, you can just run `go run main.go` to compile on the fly)*

### 3. Start Coding
Navigate to `http://localhost:8080/` in your browser. 
Open your `index.erm` file and start editing. As soon as you hit save, the changes will instantaneously reflect in your browser.

## 📄 The `.erm` Component Structure
Eronom natively serves `.erm` files as unified template components. This means your visual structure, styles, and logic live synchronously in one file.

```html
<!DOCTYPE html>
<html lang="en">
<head>
    <style>
        body { background-color: #1a1a1a; color: white; }
    </style>
</head>
<body>
    <div id="app">Hello from Eronom!</div>

    <script>
        document.addEventListener('DOMContentLoaded', () => {
            console.log("Component successfully mounted via Eronom HMR!");
        });
    </script>
</body>
</html>
```
