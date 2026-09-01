use std::fs;
use std::io::{self, IsTerminal};
use std::path::Path;
use super::spinner::{copy_dir_all, is_dir_empty, run_git_clone_with_spinner};

pub fn init_project(
    dir: &str,
    template: Option<String>,
    branch: Option<String>,
    force: bool,
    git: bool,
    no_commit: bool,
    ermcss: bool,
) -> anyhow::Result<()> {
    let start_time = std::time::Instant::now();
    let dst_dir = Path::new(dir);

    if dst_dir.exists() && !is_dir_empty(dst_dir) && !force {
        anyhow::bail!(
            "Cannot initialize project in a non-empty directory.\n\
              Run with the `--force` flag to initialize regardless."
        );
    }

    fs::create_dir_all(dst_dir)?;

    if let Some(template_str) = template.as_ref() {
        let template_url = if template_str.contains("://") {
            template_str.clone()
        } else if template_str.starts_with("github.com/") {
            format!("https://{}", template_str)
        } else {
            format!("https://github.com/{}", template_str)
        };

        let temp_dir_name = format!(
            "eronom-template-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        );
        let temp_dir = std::env::temp_dir().join(temp_dir_name);

        let mut clone_args = vec!["--depth", "1"];
        if let Some(ref b) = branch {
            clone_args.push("-b");
            clone_args.push(b);
        }
        let temp_dir_str = temp_dir.to_str().unwrap_or("");
        clone_args.push(&template_url);
        clone_args.push(temp_dir_str);

        run_git_clone_with_spinner(
            &clone_args,
            "Initializing an eronom project...",
            &format!("Failed to clone template from {}", template_url),
        )?;

        copy_dir_all(&temp_dir, dst_dir)?;

        if ermcss {
            let eronom_temp_dir_name = format!(
                "eronom-ermcss-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0)
            );
            let eronom_temp_dir = std::env::temp_dir().join(eronom_temp_dir_name);
            let eronom_temp_dir_str = eronom_temp_dir.to_str().unwrap_or("");
            let clone_args = vec!["--depth", "1", "https://github.com/eronom/eronom.git", eronom_temp_dir_str];

            if run_git_clone_with_spinner(
                &clone_args,
                "Initializing an eronom project...",
                "Failed to clone ermcss framework",
            ).is_ok() {
                let ermcss_src = eronom_temp_dir.join("libs/ermcss");
                if ermcss_src.exists() {
                    let _ = copy_dir_all(&ermcss_src, &dst_dir.join("ermcss"));
                }
            }
            let _ = fs::remove_dir_all(&eronom_temp_dir);
        }

        let _ = fs::remove_dir_all(&temp_dir);
    } else {
        // Fetch the default template (libs/init) from GitHub main branch
        let temp_dir_name = format!(
            "eronom-repo-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        );
        let temp_dir = std::env::temp_dir().join(temp_dir_name);
        let temp_dir_str = temp_dir.to_str().unwrap_or("");
        let clone_args = vec!["--depth", "1", "https://github.com/eronom/eronom.git", temp_dir_str];

        let clone_res = run_git_clone_with_spinner(
            &clone_args,
            "Initializing an eronom project...",
            "Failed to clone template from https://github.com/eronom/eronom.git",
        );

        if clone_res.is_ok() && temp_dir.join("libs/init").exists() {
            let repo_init = temp_dir.join("libs/init");
            copy_dir_all(&repo_init, dst_dir)?;

            if ermcss {
                let ermcss_src = temp_dir.join("libs/ermcss");
                if ermcss_src.exists() {
                    let _ = copy_dir_all(&ermcss_src, &dst_dir.join("ermcss"));
                }
            }
            let _ = fs::remove_dir_all(&temp_dir);
        } else {
            let _ = fs::remove_dir_all(&temp_dir);
            anyhow::bail!("Failed to clone template from https://github.com/eronom/eronom.git");
        }
    }

    if ermcss {
        let toml_path = dst_dir.join("eronom.toml");
        let mut toml_content = if toml_path.exists() {
            fs::read_to_string(&toml_path).unwrap_or_default()
        } else {
            String::new()
        };
        
        if !toml_content.contains("[ermcss]") {
            toml_content.push_str("\n[package]\nermcss = true\n\n[ermcss]\ncontent = [\n    \"./app/**/*.erm\",\n    \"./pages/**/*.erm\",\n    \"./components/**/*.erm\"\n]\n\n[ermcss.theme.extend.colors]\nprimary = \"#2563eb\"\n");
            let _ = fs::write(&toml_path, toml_content);
        }
    }

    if git {
        let mut git_init = std::process::Command::new("git");
        git_init.arg("init")
            .arg("-b").arg("main")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .current_dir(dst_dir);
        let init_status = git_init.status();
        
        if init_status.is_ok() && init_status.unwrap().success() {
            if !no_commit {
                let mut git_add = std::process::Command::new("git");
                git_add.arg("add").arg("-A")
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .current_dir(dst_dir);
                let _ = git_add.status();

                let mut git_commit = std::process::Command::new("git");
                let commit_msg = if let Some(ref t) = template {
                    format!("chore: init from {}", t)
                } else {
                    "chore: eronom init".to_string()
                };
                git_commit.arg("commit").arg("-m").arg(commit_msg)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .current_dir(dst_dir);
                let _ = git_commit.status();
            }
        }
    }

    let abs_path = dst_dir.canonicalize().unwrap_or_else(|_| dst_dir.to_path_buf());
    let dir_name = dst_dir.file_name().and_then(|n| n.to_str()).unwrap_or(dir);
    let elapsed = start_time.elapsed().as_secs_f64();

    if io::stdout().is_terminal() {
        println!("\x1b[32m✔\x1b[0m Success! Initialized {} at {} in {:.2}s", dir_name, abs_path.display(), elapsed);
    } else {
        println!("✔ Success! Initialized {} at {} in {:.2}s", dir_name, abs_path.display(), elapsed);
    }
    Ok(())
}
