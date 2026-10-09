use crate::prelude::*;

use atoman::Command;
use std::process::Stdio;

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

/// Scrapes out diff of the current working tree.
pub async fn get_git_diff(path: &Path) -> Result<String> {
    ensure_git_repo(path).await?;

    let output_staged = Command::new("git")
        .args(["diff", "--cached"])
        .current_dir(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    let diff_staged = String::from_utf8_lossy(&output_staged.stdout).to_string();

    if !diff_staged.trim().is_empty() {
        return Ok(diff_staged);
    }

    let output_head = Command::new("git")
        .args(["diff", "HEAD"])
        .current_dir(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    let diff_head = String::from_utf8_lossy(&output_head.stdout).to_string();

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
