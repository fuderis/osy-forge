use crate::prelude::*;

use atoman::Command;
use std::process::Stdio;

const DIFF_EXCLUDES: &[&str] = &[":(exclude)Cargo.lock"];

/// Checks for .git folder on the specified path.
pub async fn ensure_git_repo(path: &Path) -> Result<()> {
    let git_dir = path.join(".git");
    if !atoman::fs::metadata(&git_dir)
        .await
        .map(|m| m.is_dir() || m.is_file())
        .unwrap_or(false)
    {
        return Err(Error::Custom(format!(
            "Directory `{}` is not a git repository (missing .git).",
            path.display()
        ))
        .into());
    }
    Ok(())
}

async fn run_git_diff(path: &Path, extra_args: &[&str]) -> Result<String> {
    let mut cmd = Command::new("git");
    cmd.arg("diff")
        .args(extra_args)
        .arg("--")
        .arg(".")
        .args(DIFF_EXCLUDES)
        .current_dir(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = cmd.output().await?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Custom(format!("Git diff failed: {err}")).into());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Scrapes out diff of the current working tree.
pub async fn get_git_diff(path: &Path) -> Result<String> {
    ensure_git_repo(path).await?;

    let diff_staged = run_git_diff(path, &["--cached"]).await?;
    if !diff_staged.trim().is_empty() {
        return Ok(diff_staged);
    }

    let diff_head = run_git_diff(path, &["HEAD"]).await?;
    if diff_head.trim().is_empty() {
        return Err(Error::Custom(str!(
            "No changes detected in the repository (diff is empty)."
        ))
        .into());
    }

    Ok(diff_head)
}

/// Executes git add -A and git commit with the message.
pub async fn execute_git_commit(path: &Path, message: &str) -> Result<String> {
    ensure_git_repo(path).await?;

    let _ = Command::new("git")
        .args(["add", "-A"])
        .current_dir(path)
        .output()
        .await?;

    let output = Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Custom(format!("Git commit failed: {err}")).into());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
