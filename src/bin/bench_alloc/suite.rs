pub struct BenchmarkDef {
    pub id: &'static str,
    pub aliases: &'static [&'static str],
    pub name: &'static str,
    pub description: &'static str,
    pub default_iterations: usize,
    pub er_source: &'static str,
    pub js_source: &'static str,
}

pub fn get_benchmark_suite() -> Vec<BenchmarkDef> {
    vec![
        BenchmarkDef {
            id: "object_alloc",
            aliases: &["alloc", "objects", "object"],
            name: "Object & Array Allocation Lifecycle",
            description: "50,000 loop iterations allocating objects, arrays, resizing & string templating",
            default_iterations: 30,
            er_source: r#"
for i in 1..50000 {
    let arr = [i, i + 1, i + 2]
    arr.push(i + 3)
    let pop_val = arr.pop()
    let obj = { a: arr, b: i }
    let s = "num: {i}"
    let dummy = obj.a
}
"#,
            js_source: r#"
for (let i = 1; i < 50000; i++) {
    let arr = [i, i + 1, i + 2];
    arr.push(i + 3);
    let pop_val = arr.pop();
    let obj = { a: arr, b: i };
    let s = `num: ${i}`;
    let dummy = obj.a;
}
"#,
        },
        BenchmarkDef {
            id: "fibonacci",
            aliases: &["fib"],
            name: "Recursive Fibonacci (Call Frame & Stack Dispatch)",
            description: "Deep recursive fib(28) testing function call dispatch, call frames & integer math",
            default_iterations: 20,
            er_source: r#"
fn fib(n) {
    if (n <= 1) {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}
let res = fib(28);
"#,
            js_source: r#"
function fib(n) {
    if (n <= 1) {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}
let res = fib(28);
"#,
        },
        BenchmarkDef {
            id: "binary_trees",
            aliases: &["trees", "tree", "binary_tree"],
            name: "Binary Trees (GC Object Graph & Tree Traversal)",
            description: "Bottom-up binary tree creation (depth=12) & recursive item checksums",
            default_iterations: 20,
            er_source: r#"
fn bottomUpTree(depth) {
    if (depth <= 0) {
        return { left: null, right: null };
    }
    return {
        left: bottomUpTree(depth - 1),
        right: bottomUpTree(depth - 1)
    };
}

fn itemCheck(node) {
    if (node.left == null) {
        return 1;
    }
    return 1 + itemCheck(node.left) + itemCheck(node.right);
}

let maxDepth = 12;
let stretchDepth = maxDepth + 1;
let check = itemCheck(bottomUpTree(stretchDepth));
let longLivedTree = bottomUpTree(maxDepth);
let longLivedCheck = itemCheck(longLivedTree);
"#,
            js_source: r#"
function bottomUpTree(depth) {
    if (depth <= 0) {
        return { left: null, right: null };
    }
    return {
        left: bottomUpTree(depth - 1),
        right: bottomUpTree(depth - 1)
    };
}

function itemCheck(node) {
    if (node.left === null) {
        return 1;
    }
    return 1 + itemCheck(node.left) + itemCheck(node.right);
}

let maxDepth = 12;
let stretchDepth = maxDepth + 1;
let check = itemCheck(bottomUpTree(stretchDepth));
let longLivedTree = bottomUpTree(maxDepth);
let longLivedCheck = itemCheck(longLivedTree);
"#,
        },
        BenchmarkDef {
            id: "sieve_primes",
            aliases: &["sieve", "primes", "prime"],
            name: "Sieve of Eratosthenes (Array Mutation & Indexing)",
            description: "Prime sieve computing all primes up to 30,000 with inner loop cross-off",
            default_iterations: 20,
            er_source: r#"
fn sieve(limit) {
    let is_prime = [];
    for i in 0..limit {
        is_prime.push(1);
    }
    let p = 2;
    while (p * p < limit) {
        if (is_prime[p] == 1) {
            let k = p * p;
            while (k < limit) {
                is_prime[k] = 0;
                k = k + p;
            }
        }
        p = p + 1;
    }
    let count = 0;
    for i in 2..limit {
        if (is_prime[i] == 1) {
            count = count + 1;
        }
    }
    return count;
}
let primes = sieve(30000);
"#,
            js_source: r#"
function sieve(limit) {
    let is_prime = [];
    for (let i = 0; i < limit; i++) {
        is_prime.push(1);
    }
    let p = 2;
    while (p * p < limit) {
        if (is_prime[p] === 1) {
            let k = p * p;
            while (k < limit) {
                is_prime[k] = 0;
                k = k + p;
            }
        }
        p = p + 1;
    }
    let count = 0;
    for (let i = 2; i < limit; i++) {
        if (is_prime[i] === 1) {
            count = count + 1;
        }
    }
    return count;
}
let primes = sieve(30000);
"#,
        },
        BenchmarkDef {
            id: "matrix_mult",
            aliases: &["matrix", "matmul"],
            name: "Matrix Multiplication (2D Array Math & 3-Level Loops)",
            description: "45x45 2D matrix multiplication with O(N^3) nested loop arithmetic",
            default_iterations: 20,
            er_source: r#"
fn makeMatrix(rows, cols, initial) {
    let mat = [];
    for r in 0..rows {
        let row = [];
        for c in 0..cols {
            row.push(initial + r + c);
        }
        mat.push(row);
    }
    return mat;
}

fn matrixMultiply(a, b, n) {
    let res = [];
    for i in 0..n {
        let row = [];
        for j in 0..n {
            let sum = 0;
            for k in 0..n {
                sum = sum + a[i][k] * b[k][j];
            }
            row.push(sum);
        }
        res.push(row);
    }
    return res;
}

let n = 45;
let a = makeMatrix(n, n, 1);
let b = makeMatrix(n, n, 2);
let c = matrixMultiply(a, b, n);
"#,
            js_source: r#"
function makeMatrix(rows, cols, initial) {
    let mat = [];
    for (let r = 0; r < rows; r++) {
        let row = [];
        for (let c = 0; c < cols; c++) {
            row.push(initial + r + c);
        }
        mat.push(row);
    }
    return mat;
}

function matrixMultiply(a, b, n) {
    let res = [];
    for (let i = 0; i < n; i++) {
        let row = [];
        for (let j = 0; j < n; j++) {
            let sum = 0;
            for (let k = 0; k < n; k++) {
                sum = sum + a[i][k] * b[k][j];
            }
            row.push(sum);
        }
        res.push(row);
    }
    return res;
}

let n = 45;
let a = makeMatrix(n, n, 1);
let b = makeMatrix(n, n, 2);
let c = matrixMultiply(a, b, n);
"#,
        },
        BenchmarkDef {
            id: "mandelbrot",
            aliases: &["mandel", "fractal"],
            name: "Mandelbrot Computation (Hot Floating-Point Loops)",
            description: "100x100 grid Mandelbrot escape-time calculation with 100 max iterations",
            default_iterations: 20,
            er_source: r#"
fn mandelbrot(width, height, max_iter) {
    let checksum = 0;
    for y in 0..height {
        let ci = (y * 2.0 / height) - 1.0;
        for x in 0..width {
            let cr = (x * 3.0 / width) - 2.0;
            let zr = 0.0;
            let zi = 0.0;
            let iter = 0;
            while (zr * zr + zi * zi <= 4.0 and iter < max_iter) {
                let temp = zr * zr - zi * zi + cr;
                zi = 2.0 * zr * zi + ci;
                zr = temp;
                iter = iter + 1;
            }
            checksum = checksum + iter;
        }
    }
    return checksum;
}
let sum = mandelbrot(100, 100, 100);
"#,
            js_source: r#"
function mandelbrot(width, height, max_iter) {
    let checksum = 0;
    for (let y = 0; y < height; y++) {
        let ci = (y * 2.0 / height) - 1.0;
        for (let x = 0; x < width; x++) {
            let cr = (x * 3.0 / width) - 2.0;
            let zr = 0.0;
            let zi = 0.0;
            let iter = 0;
            while (zr * zr + zi * zi <= 4.0 && iter < max_iter) {
                let temp = zr * zr - zi * zi + cr;
                zi = 2.0 * zr * zi + ci;
                zr = temp;
                iter = iter + 1;
            }
            checksum += iter;
        }
    }
    return checksum;
}
let sum = mandelbrot(100, 100, 100);
"#,
        },
        BenchmarkDef {
            id: "data_pipeline",
            aliases: &["pipeline", "data", "records"],
            name: "Data Pipeline & Aggregation (Realistic Backend Workload)",
            description: "Generating 5,000 user records, filtering, transforming fields & computing aggregates",
            default_iterations: 20,
            er_source: r#"
fn runPipeline(count) {
    let users = [];
    for i in 0..count {
        let active = 0;
        if (i % 2 == 0) {
            active = 1;
        }
        let user = {
            id: i,
            name: "user_{i}",
            score: (i * 17) % 100,
            active: active
        };
        users.push(user);
    }
    
    let totalScore = 0;
    let activeCount = 0;
    let highScorers = [];
    for i in 0..count {
        let u = users[i];
        if (u.active == 1) {
            totalScore = totalScore + u.score;
            activeCount = activeCount + 1;
            if (u.score > 50) {
                highScorers.push(u);
            }
        }
    }
    let hs_len = highScorers.length;
    let res = {
        total: count,
        active: activeCount,
        sumScore: totalScore,
        highScoreCount: hs_len
    };
    return res;
}
let stats = runPipeline(5000);
"#,
            js_source: r#"
function runPipeline(count) {
    let users = [];
    for (let i = 0; i < count; i++) {
        let active = 0;
        if (i % 2 === 0) {
            active = 1;
        }
        let user = {
            id: i,
            name: `user_${i}`,
            score: (i * 17) % 100,
            active: active
        };
        users.push(user);
    }
    
    let totalScore = 0;
    let activeCount = 0;
    let highScorers = [];
    for (let i = 0; i < count; i++) {
        let u = users[i];
        if (u.active === 1) {
            totalScore = totalScore + u.score;
            activeCount = activeCount + 1;
            if (u.score > 50) {
                highScorers.push(u);
            }
        }
    }
    let hs_len = highScorers.length;
    let res = {
        total: count,
        active: activeCount,
        sumScore: totalScore,
        highScoreCount: hs_len
    };
    return res;
}
let stats = runPipeline(5000);
"#,
        },
    ]
}
