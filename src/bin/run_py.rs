use std::process::{Command, Child};
use std::io::{self, BufRead, BufReader};
use std::thread;

struct ChildGuard {
    child: Child,
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        println!("\n[Rust] Shutting down Python API server...");
        let _ = self.child.kill();
        let _ = self.child.wait();
        println!("[Rust] Python API server stopped.");
    }
}

fn main() -> io::Result<()> {
    println!("==================================================");
    println!("        Python Todo API Server Runner (Rust)      ");
    println!("==================================================");
    println!("[Rust] Starting Python server...");

    // Try executing using the virtual environment Python first
    let venv_python = ".venv/bin/python";
    let child = Command::new(venv_python)
        .arg("main.py")
        .env("PYTHONUNBUFFERED", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();

    // Fallback to system "python3" or "python" if .venv Python is not found
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            if e.kind() == io::ErrorKind::NotFound {
                Command::new("python3")
                    .arg("main.py")
                    .env("PYTHONUNBUFFERED", "1")
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .or_else(|_| {
                        Command::new("python")
                            .arg("main.py")
                            .env("PYTHONUNBUFFERED", "1")
                            .stdout(std::process::Stdio::piped())
                            .stderr(std::process::Stdio::piped())
                            .spawn()
                    })?
            } else {
                return Err(e);
            }
        }
    };

    // Pipe stdout to custom output logger thread
    let stdout = child.stdout.take().expect("Failed to open stdout");
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            if let Ok(l) = line {
                println!("[Py Log] {}", l);
            }
        }
    });

    // Pipe stderr to custom output logger thread
    let stderr = child.stderr.take().expect("Failed to open stderr");
    thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            if let Ok(l) = line {
                eprintln!("[Py Err] {}", l);
            }
        }
    });

    let _guard = ChildGuard { child };

    println!("[Rust] Python API server is running at http://localhost:8080");
    println!("[Rust] Press ENTER to stop the server...");

    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);

    Ok(())
}
