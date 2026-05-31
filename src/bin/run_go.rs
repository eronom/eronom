use std::process::{Command, Child};
use std::io::{self, BufRead, BufReader};
use std::thread;
use std::path::Path;

struct ChildGuard {
    child: Child,
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        println!("\n[Rust] Shutting down Go API server...");
        let _ = self.child.kill();
        let _ = self.child.wait();
        
        // Clean up the compiled binary
        if Path::new("main_go").exists() {
            let _ = std::fs::remove_file("main_go");
            println!("[Rust] Removed temporary binary ./main_go");
        }
        println!("[Rust] Go API server stopped.");
    }
}

fn main() -> io::Result<()> {
    println!("==================================================");
    println!("          Go Todo API Server Runner (Rust)        ");
    println!("==================================================");
    println!("[Rust] Compiling main.go...");

    let build_status = Command::new("go")
        .args(&["build", "-o", "main_go", "main.go"])
        .status()?;

    if !build_status.success() {
        eprintln!("[Rust] Error: Failed to compile main.go");
        std::process::exit(1);
    }

    println!("[Rust] Successfully compiled main.go -> ./main_go");
    println!("[Rust] Starting Go server...");

    let mut child = Command::new("./main_go")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    // Pipe stdout to custom output logger thread
    let stdout = child.stdout.take().expect("Failed to open stdout");
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            if let Ok(l) = line {
                println!("[Go Log] {}", l);
            }
        }
    });

    // Pipe stderr to custom output logger thread
    let stderr = child.stderr.take().expect("Failed to open stderr");
    thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            if let Ok(l) = line {
                eprintln!("[Go Err] {}", l);
            }
        }
    });

    let _guard = ChildGuard { child };

    println!("[Rust] Go API server is running at http://localhost:8080");
    println!("[Rust] Press ENTER to stop the server...");

    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);

    Ok(())
}
