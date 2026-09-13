use std::fs;
use std::io::{self, IsTerminal};
use std::path::Path;
use super::spinner::{copy_dir_all, is_dir_empty, run_git_clone_with_spinner};

fn find_local_template(name: &str) -> Option<std::path::PathBuf> {
    let mut candidates = vec![
        Path::new("libs/template").join(name),
        Path::new("libs/templates").join(name),
        Path::new("template").join(name),
        Path::new("templates").join(name),
    ];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("libs/template").join(name));
            candidates.push(parent.join("libs/templates").join(name));
            candidates.push(parent.join("template").join(name));
            candidates.push(parent.join("templates").join(name));
            if let Some(grandparent) = parent.parent() {
                candidates.push(grandparent.join("libs/template").join(name));
                candidates.push(grandparent.join("libs/templates").join(name));
                candidates.push(grandparent.join("template").join(name));
                candidates.push(grandparent.join("templates").join(name));
            }
        }
    }
    for cand in candidates {
        if cand.is_dir() && cand.join("app").exists() {
            return Some(cand);
        }
    }
    None
}

fn find_local_init() -> Option<std::path::PathBuf> {
    let mut candidates = vec![
        Path::new("libs/init").to_path_buf(),
    ];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("libs/init"));
            if let Some(grandparent) = parent.parent() {
                candidates.push(grandparent.join("libs/init"));
            }
        }
    }
    for cand in candidates {
        if cand.is_dir() && cand.join("app").exists() {
            return Some(cand);
        }
    }
    None
}

pub fn init_project(
    dir: &str,
    template: Option<String>,
    branch: Option<String>,
    force: bool,
    git: bool,
    no_commit: bool,
    ermcss: bool,
    offline: bool,
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
        let is_local_dir = Path::new(template_str).is_dir() && Path::new(template_str).join("app").exists();
        let is_remote_repo = template_str.contains("://") || template_str.contains('/') || template_str.starts_with("github.com/");

        if is_local_dir {
            copy_dir_all(Path::new(template_str), dst_dir)?;
        } else if !is_remote_repo {
            // Named official template (e.g. "basic")
            if offline {
                if let Some(local_path) = find_local_template(template_str) {
                    copy_dir_all(&local_path, dst_dir)?;
                } else {
                    anyhow::bail!(
                        "Template '{}' not found locally in `libs/template/` or `template/`. Run without `--offline` to clone from GitHub.",
                        template_str
                    );
                }
            } else {
                let temp_dir_name = format!(
                    "eronom-template-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or(0)
                );
                let temp_dir = std::env::temp_dir().join(temp_dir_name);
                let temp_dir_str = temp_dir.to_str().unwrap_or("");

                let mut clone_args = vec!["--depth", "1"];
                if let Some(ref b) = branch {
                    clone_args.push("-b");
                    clone_args.push(b);
                }
                clone_args.push("https://github.com/eronom/eronom.git");
                clone_args.push(temp_dir_str);

                let clone_res = run_git_clone_with_spinner(
                    &clone_args,
                    "Initializing an eronom project...",
                    "Failed to clone repository from https://github.com/eronom/eronom.git",
                );

                let cand1 = temp_dir.join("libs/template").join(template_str);
                let cand2 = temp_dir.join("libs/templates").join(template_str);
                let cand3 = temp_dir.join("template").join(template_str);
                let cand4 = temp_dir.join("templates").join(template_str);

                if clone_res.is_ok() && cand1.is_dir() && cand1.join("app").exists() {
                    copy_dir_all(&cand1, dst_dir)?;
                } else if clone_res.is_ok() && cand2.is_dir() && cand2.join("app").exists() {
                    copy_dir_all(&cand2, dst_dir)?;
                } else if clone_res.is_ok() && cand3.is_dir() && cand3.join("app").exists() {
                    copy_dir_all(&cand3, dst_dir)?;
                } else if clone_res.is_ok() && cand4.is_dir() && cand4.join("app").exists() {
                    copy_dir_all(&cand4, dst_dir)?;
                } else if let Some(local_path) = find_local_template(template_str) {
                    // Fallback to local template if remote does not yet have it
                    copy_dir_all(&local_path, dst_dir)?;
                } else {
                    let _ = fs::remove_dir_all(&temp_dir);
                    anyhow::bail!(
                        "Template '{}' not found in https://github.com/eronom/eronom.git under `libs/template/` or `template/`",
                        template_str
                    );
                }

                if ermcss {
                    let ermcss_src = temp_dir.join("libs/ermcss");
                    if ermcss_src.exists() {
                        let _ = copy_dir_all(&ermcss_src, &dst_dir.join("ermcss"));
                    }
                }
                let _ = fs::remove_dir_all(&temp_dir);
            }
        } else {
            // Remote git repository URL / slug
            if offline {
                anyhow::bail!("Cannot clone remote template in --offline mode: {}", template_str);
            } else {
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
            }
        }
    } else if offline {
        // When --offline is specified and no template, use local libs/init
        if let Some(init_path) = find_local_init() {
            copy_dir_all(&init_path, dst_dir)?;
            if ermcss {
                let ermcss_cand = init_path.parent().unwrap_or(Path::new("")).join("ermcss");
                if ermcss_cand.exists() {
                    let _ = copy_dir_all(&ermcss_cand, &dst_dir.join("ermcss"));
                }
            }
        } else {
            anyhow::bail!(
                "Could not find local template files in `libs/init`. Run without `--offline` to clone from GitHub."
            );
        }
    } else {
        // Default: Always fetch the latest template (libs/init) from GitHub main branch
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
        } else if let Some(init_path) = find_local_init() {
            // Fallback to local init if clone fails
            copy_dir_all(&init_path, dst_dir)?;
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
