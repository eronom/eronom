use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

pub fn is_dir_empty(path: &Path) -> bool {
    if let Ok(mut entries) = fs::read_dir(path) {
        entries.next().is_none()
    } else {
        true
    }
}

pub fn copy_dir_all(src: &Path, dst: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let file_name = entry.file_name();
        
        // Exclude the compiled eronom binary and git directory if they are in the source folder
        if file_name == "eronom" || file_name == ".git" {
            continue;
        }

        let dst_path = dst.join(&file_name);
        if file_type.is_dir() {
            copy_dir_all(&entry.path(), &dst_path)?;
        } else {
            fs::copy(&entry.path(), &dst_path)?;
        }
    }
    Ok(())
}

pub struct Spinner {
    running: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    is_tty: bool,
}

impl Spinner {
    pub fn start(msg: impl Into<String>) -> Self {
        let msg = msg.into();
        let is_tty = io::stdout().is_terminal();
        let running = Arc::new(AtomicBool::new(true));

        let handle = if is_tty {
            let running_clone = running.clone();
            let msg_clone = msg.clone();
            Some(thread::spawn(move || {
                let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
                let mut i = 0;
                let mut stdout = io::stdout();
                while running_clone.load(Ordering::Relaxed) {
                    let frame = frames[i % frames.len()];
                    let _ = write!(stdout, "\r\x1b[K\x1b[36m{}\x1b[0m {}", frame, msg_clone);
                    let _ = stdout.flush();
                    i += 1;
                    thread::sleep(std::time::Duration::from_millis(80));
                }
            }))
        } else {
            let mut stdout = io::stdout();
            let _ = writeln!(stdout, "{}...", msg);
            let _ = stdout.flush();
            None
        };

        Self {
            running,
            handle,
            is_tty,
        }
    }

    pub fn finish_and_clear(&mut self) {
        self.stop();
        if self.is_tty {
            let mut stdout = io::stdout();
            let _ = write!(stdout, "\r\x1b[K");
            let _ = stdout.flush();
        }
    }

    pub fn finish_with_failure(&mut self, fail_msg: &str) {
        self.stop();
        let mut stdout = io::stdout();
        if self.is_tty {
            let _ = writeln!(stdout, "\r\x1b[K\x1b[31m✖\x1b[0m {}", fail_msg);
        } else {
            let _ = writeln!(stdout, "✖ {}", fail_msg);
        }
        let _ = stdout.flush();
    }

    fn stop(&mut self) {
        if self.handle.is_some() {
            self.running.store(false, Ordering::Relaxed);
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn run_git_clone_with_spinner(
    args: &[&str],
    start_msg: &str,
    fail_msg: &str,
) -> anyhow::Result<()> {
    let mut spinner = Spinner::start(start_msg);

    let mut git_clone = std::process::Command::new("git");
    git_clone.arg("clone");
    for arg in args {
        git_clone.arg(arg);
    }
    git_clone.stdout(std::process::Stdio::null());
    git_clone.stderr(std::process::Stdio::null());

    match git_clone.status() {
        Ok(status) if status.success() => {
            spinner.finish_and_clear();
            Ok(())
        }
        Ok(_) => {
            spinner.finish_with_failure(fail_msg);
            anyhow::bail!("{}", fail_msg);
        }
        Err(e) => {
            spinner.finish_with_failure(fail_msg);
            anyhow::bail!("{}: {}", fail_msg, e);
        }
    }
}
