use std::process::{Command, Child};
use std::io::{self, BufRead, BufReader};
use std::thread;
use std::path::Path;

struct ChildGuard {
    child: Child,
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        println!("\n[Rust] Shutting down Go API server (air)...");
        let _ = self.child.kill();
        let _ = self.child.wait();
        
        // Clean up any compiled binaries
        if Path::new("main_go").exists() {
            let _ = std::fs::remove_file("main_go");
            println!("[Rust] Removed temporary binary ./main_go");
        }
        if Path::new("tmp/main").exists() {
            let _ = std::fs::remove_file("tmp/main");
            println!("[Rust] Removed temporary binary ./tmp/main");
        }
        if Path::new("tmp").exists() {
            let _ = std::fs::remove_dir_all("tmp");
            println!("[Rust] Cleaned up temporary directory ./tmp");
        }
        println!("[Rust] Go API server stopped.");
    }
}

fn main() -> io::Result<()> {
    println!("==================================================");
    println!("          Go Todo API Server Runner (Rust)        ");
    println!("==================================================");
    println!("[Rust] Locating 'air' binary...");

    let air_bin = if Command::new("air")
        .arg("-v")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
    {
        "air".to_string()
    } else {
        let home = std::env::var("HOME").unwrap_or_default();
        let fallback = format!("{}/go/bin/air", home);
        if Path::new(&fallback).exists() {
            fallback
        } else {
            eprintln!("[Rust] Error: 'air' binary not found in PATH or at $HOME/go/bin/air.");
            eprintln!("[Rust] Please install it using: go install github.com/air-verse/air@latest");
            std::process::exit(1);
        }
    };

    println!("[Rust] Starting Go server with air ({})...", air_bin);

    let mut child = Command::new(&air_bin)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    // Pipe stdout to custom output logger thread
    let stdout = child.stdout.take().expect("Failed to open stdout");
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            if let Ok(l) = line {
                println!("[Air Log] {}", l);
            }
        }
    });

    // Pipe stderr to custom output logger thread
    let stderr = child.stderr.take().expect("Failed to open stderr");
    thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            if let Ok(l) = line {
                eprintln!("[Air Err] {}", l);
            }
        }
    });

    let _guard = ChildGuard { child };

    println!("[Rust] Go API server is running with live reload at http://localhost:8080");
    println!("[Rust] Press ENTER to stop the server...");

    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);

    Ok(())
}
