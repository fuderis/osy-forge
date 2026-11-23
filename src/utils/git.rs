use crate::prelude::*;

const DIFF_EXCLUDES: &str = "':(exclude)Cargo.lock'";

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

async fn run_script(tx: &Sender<Bytes>, cmd: String) -> Result<CommandResult> {
    match Dialog::script(tx, cmd.clone(), Duration::from_secs(120)).await? {
        Some(res) => Ok(res),
        None => Err(Error::OperationTimeout.into()),
    }
}

async fn run_git(tx: &Sender<Bytes>, path: &Path, git_args: &str) -> Result<CommandResult> {
    let dir_q = shell_quote(&path.to_string_lossy());
    let cmd = format!("cd {dir_q} && git {git_args}");
    run_script(tx, cmd).await
}

/// Checks for .git folder or file on the client repository path.
pub async fn ensure_git_repo(tx: &Sender<Bytes>, path: &Path) -> Result<()> {
    let dir_q = shell_quote(&path.to_string_lossy());
    let cmd = format!("test -d {dir_q}/.git -o -f {dir_q}/.git");
    let res = run_script(tx, cmd).await?;
    if !res.success {
        return Err(Error::Custom(format!(
            "Directory `{}` is not a git repository (missing .git).",
            path.display()
        ))
        .into());
    }
    Ok(())
}

async fn run_git_diff(tx: &Sender<Bytes>, path: &Path, extra_args: &str) -> Result<String> {
    let args = format!("diff {extra_args} -- . {DIFF_EXCLUDES}");
    let res = run_git(tx, path, &args).await?;
    if !res.success {
        return Err(Error::Custom(format!("Git diff failed: {}", res.stderr)).into());
    }
    Ok(res.stdout)
}

/// Checks whether there are unstaged or untracked changes not yet added to the index.
pub async fn has_unstaged_changes(tx: &Sender<Bytes>, path: &Path) -> Result<bool> {
    ensure_git_repo(tx, path).await?;

    let res = run_git(tx, path, "status --porcelain").await?;
    if !res.success {
        return Err(Error::Custom(format!("Git status failed: {}", res.stderr)).into());
    }

    let has_unstaged = res.stdout.lines().any(|line| {
        if line.len() < 2 {
            return false;
        }
        let bytes = line.as_bytes();
        (bytes[0] == b'?' && bytes[1] == b'?') || bytes[1] != b' '
    });

    Ok(has_unstaged)
}

/// Checks whether there are any staged, unstaged, or untracked changes in the working tree.
pub async fn has_uncommitted_changes(tx: &Sender<Bytes>, path: &Path) -> Result<bool> {
    ensure_git_repo(tx, path).await?;

    let res = run_git(tx, path, "status --porcelain").await?;
    if !res.success {
        return Err(Error::Custom(format!("Git status failed: {}", res.stderr)).into());
    }

    Ok(!res.stdout.trim().is_empty())
}

/// Stages all changes in the repository (`git add -A`).
pub async fn execute_git_add_all(tx: &Sender<Bytes>, path: &Path) -> Result<()> {
    ensure_git_repo(tx, path).await?;

    let res = run_git(tx, path, "add -A").await?;
    if !res.success {
        return Err(Error::Custom(format!("Git add failed: {}", res.stderr)).into());
    }

    Ok(())
}

/// Scrapes out diff of the current working tree (or staged changes if `staged_only` is true).
pub async fn get_git_diff(tx: &Sender<Bytes>, path: &Path, staged_only: bool) -> Result<String> {
    ensure_git_repo(tx, path).await?;

    let diff_staged = run_git_diff(tx, path, "--cached").await?;
    if !diff_staged.trim().is_empty() {
        return Ok(diff_staged);
    }

    if staged_only {
        return Err(Error::Custom(str!("No staged changes in the index (diff is empty).")).into());
    }

    let diff_head = run_git_diff(tx, path, "HEAD").await?;
    if diff_head.trim().is_empty() {
        return Err(Error::Custom(str!(
            "No changes detected in the repository (diff is empty)."
        ))
        .into());
    }

    Ok(diff_head)
}

/// Executes `git commit` with the message (stages all if `stage_all` is true).
pub async fn execute_git_commit(
    tx: &Sender<Bytes>,
    path: &Path,
    message: &str,
    stage_all: bool,
) -> Result<String> {
    ensure_git_repo(tx, path).await?;

    if stage_all {
        execute_git_add_all(tx, path).await?;
    }

    let msg_q = shell_quote(message);
    let res = run_git(tx, path, &format!("commit -m {msg_q}")).await?;
    if !res.success {
        return Err(Error::Custom(format!("Git commit failed: {}", res.stderr)).into());
    }

    Ok(res.stdout.trim().to_string())
}

/// Pushes current HEAD to `remote` / `branch` (defaults to `origin` / `main`).
pub async fn execute_git_push(
    tx: &Sender<Bytes>,
    path: &Path,
    remote: &str,
    branch: &str,
) -> Result<String> {
    ensure_git_repo(tx, path).await?;

    let remote_q = shell_quote(remote);
    let refspec_q = shell_quote(&format!("HEAD:{branch}"));
    let res = run_git(tx, path, &format!("push {remote_q} {refspec_q}")).await?;
    if !res.success {
        return Err(Error::Custom(format!("Git push failed: {}", res.stderr)).into());
    }

    let out = res.stdout.trim();
    if out.is_empty() {
        Ok(res.stderr.trim().to_string())
    } else {
        Ok(out.to_string())
    }
}

/// Creates a tag and optionally pushes it to `remote`.
pub async fn execute_git_tag(
    tx: &Sender<Bytes>,
    path: &Path,
    tag: &str,
    message: Option<&str>,
    remote: Option<&str>,
) -> Result<String> {
    ensure_git_repo(tx, path).await?;

    let tag_q = shell_quote(tag);
    if git_tag_exists(tx, path, tag).await? {
        return Err(Error::Custom(format!("Tag `{tag}` already exists in repository.")).into());
    }

    let tag_args = match message {
        Some(msg) => {
            let msg_q = shell_quote(msg);
            format!("tag -a {tag_q} -m {msg_q}")
        }
        None => format!("tag {tag_q}"),
    };

    let tag_out = run_git(tx, path, &tag_args).await?;
    if !tag_out.success {
        return Err(Error::Custom(format!("Git tag failed: {}", tag_out.stderr)).into());
    }

    let mut result = format!("Created tag `{tag}`");

    if let Some(rem) = remote {
        let rem_q = shell_quote(rem);
        let push_out = run_git(tx, path, &format!("push {rem_q} {tag_q}")).await?;
        if !push_out.success {
            return Err(
                Error::Custom(format!("Git push tag `{tag}` failed: {}", push_out.stderr)).into(),
            );
        }

        result.push_str(&format!(" and pushed to `{rem}`"));
    }

    Ok(result)
}

/// Checks whether a tag with the given name already exists in the repository.
pub async fn git_tag_exists(tx: &Sender<Bytes>, path: &Path, tag: &str) -> Result<bool> {
    ensure_git_repo(tx, path).await?;

    let tag_q = shell_quote(tag);
    let existing = run_git(tx, path, &format!("tag -l {tag_q}")).await?;
    if !existing.success {
        return Err(Error::Custom(format!("Git tag lookup failed: {}", existing.stderr)).into());
    }

    Ok(!existing.stdout.trim().is_empty())
}
