use crate::{
    prelude::*,
    utils::{self, cargo},
};

use anylm::{
    IntoSchema, Messages, Schema,
    completions::{Chunk, Completions},
};

#[derive(Debug, Deserialize, Schema)]
pub struct DiffAction {
    /// Optional target repository directory path. Defaults to current working path if omitted.
    pub path: Option<String>,
}

#[derive(Debug, Deserialize, Schema)]
pub struct CommitAction {
    /// Optional target repository directory path. Defaults to current working path if omitted.
    pub path: Option<String>,
}

#[derive(Debug, Deserialize, Schema)]
pub struct PushAction {
    /// Optional target repository directory path. Defaults to current working path if omitted.
    pub path: Option<String>,
    /// Target branch name. Defaults to `main` if omitted.
    pub branch: Option<String>,
    /// Git remote name. Defaults to `origin` if omitted.
    pub remote: Option<String>,
}

#[derive(Debug, Deserialize, Schema)]
pub struct TagAction {
    /// Optional target repository directory path. Defaults to current working path if omitted.
    pub path: Option<String>,
    /// Explicit tag name, e.g. `v0.2.4`. If omitted, resolved from Cargo.toml.
    pub tag: Option<String>,
    /// Optional annotated tag message.
    pub message: Option<String>,
    /// Push tag to remote after creation. Defaults to `true`.
    pub push: Option<bool>,
}

#[derive(Debug, Deserialize, Schema)]
pub struct ReleaseAction {
    /// Optional target repository directory path. Defaults to current working path if omitted.
    pub path: Option<String>,
    /// Target branch name to push. Defaults to `main` if omitted.
    pub branch: Option<String>,
    /// Optional explicit version or tag override (e.g. `0.2.4` or `v0.2.4`).
    pub version: Option<String>,
}

#[log(path = %query.payload.path.as_deref().unwrap_or("default"))]
pub async fn handle_diff(tx: Sender<Bytes>, query: ToolQuery<DiffAction>) -> Result<()> {
    let ToolQuery {
        current_path,
        payload,
    } = query;

    let target_path = resolve_target_path(payload.path, current_path)?;

    tx.send(Event::Thinking(
        "Checking git repository and diff...".into(),
    ))?;
    let diff = utils::get_git_diff(&tx, &target_path, false).await?;

    tx.send(Event::Thinking("Analyzing diff via LLM...".into()))?;

    let cfg = Config::get();
    let messages = Messages::new()
        .system(vec![cfg.llm.diff_prompt().into()])
        .user(vec![
            format!("Analyze the following git diff:\n\n```diff\n{diff}\n```").into(),
        ])
        .wrap();

    #[derive(Deserialize, Schema)]
    struct DiffAnalysis {
        /// Short summary of structural and logical changes.
        pub summary: String,
    }

    let mut response = Completions::try_from(cfg.llm.diff_options())?
        .schema(DiffAnalysis::schema())
        .send(messages)
        .await?;

    let mut json_str = String::new();
    while let Some(chunk) = response.next().await {
        if let Chunk::Text(text) = chunk? {
            json_str.push_str(&text);
        }
    }

    let analysis: DiffAnalysis = serde_json::from_str(&json_str)
        .map_err(|e| Error::Titled("Failed to parse LLM diff response".into(), e.into()))?;

    info!(
        "Git diff successfully analyzed for path `{}`",
        target_path.display()
    );
    tx.send(Event::Answer(analysis.summary))?;
    Ok(())
}

#[log(path = %query.payload.path.as_deref().unwrap_or("default"))]
pub async fn handle_commit(tx: Sender<Bytes>, query: ToolQuery<CommitAction>) -> Result<()> {
    let ToolQuery {
        current_path,
        payload,
    } = query;

    let target_path = resolve_target_path(payload.path, current_path)?;

    match run_commit_flow(&tx, &target_path).await? {
        CommitFlowResult::Committed(msg) => {
            info!("Git commit executed in `{}`", target_path.display());
            tx.send(Event::Answer(msg))?;
        }
        CommitFlowResult::Cancelled(reason) => {
            tx.send(Event::Answer(reason))?;
        }
    }

    Ok(())
}

#[log(path = %query.payload.path.as_deref().unwrap_or("default"))]
pub async fn handle_push(tx: Sender<Bytes>, query: ToolQuery<PushAction>) -> Result<()> {
    let ToolQuery {
        current_path,
        payload,
    } = query;

    let target_path = resolve_target_path(payload.path, current_path)?;
    let branch = payload
        .branch
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("main");
    let remote = payload
        .remote
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("origin");

    tx.send(Event::Thinking(format!(
        "Pushing `{branch}` to `{remote}`..."
    )))?;

    let output = utils::execute_git_push(&tx, &target_path, remote, branch).await?;
    info!(
        "Pushed `{branch}` to `{remote}` in `{}`",
        target_path.display()
    );

    let msg = if output.is_empty() {
        format!("Pushed `{branch}` to `{remote}`.")
    } else {
        format!("Pushed `{branch}` to `{remote}`:\n\n```\n{output}\n```")
    };

    tx.send(Event::Answer(msg))?;
    Ok(())
}

#[log(path = %query.payload.path.as_deref().unwrap_or("default"))]
pub async fn handle_tag(tx: Sender<Bytes>, query: ToolQuery<TagAction>) -> Result<()> {
    let ToolQuery {
        current_path,
        payload,
    } = query;

    let target_path = resolve_target_path(payload.path, current_path)?;
    let should_push = payload.push.unwrap_or(true);

    tx.send(Event::Thinking("Resolving tag name...".into()))?;

    let tag = match payload
        .tag
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(t) => normalize_tag(t),
        None => {
            let Some((ver, _)) = pick_release_version(&tx, &target_path, None).await? else {
                tx.send(Event::Answer(
                    "Tag operation was cancelled by the user.".to_string(),
                ))?;
                return Ok(());
            };
            normalize_tag(&ver)
        }
    };

    tx.send(Event::Thinking(format!("Creating tag `{tag}`...")))?;
    let remote = should_push.then_some("origin");
    let status =
        utils::execute_git_tag(&tx, &target_path, &tag, payload.message.as_deref(), remote).await?;

    info!("Tag `{tag}` processed in `{}`", target_path.display());
    tx.send(Event::Answer(format!("{status}.")))?;
    Ok(())
}

#[log(path = %query.payload.path.as_deref().unwrap_or("default"))]
pub async fn handle_release(tx: Sender<Bytes>, query: ToolQuery<ReleaseAction>) -> Result<()> {
    let ToolQuery {
        current_path,
        payload,
    } = query;

    let target_path = resolve_target_path(payload.path, current_path)?;
    let branch = payload
        .branch
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("main");

    utils::ensure_git_repo(&tx, &target_path).await?;

    let mut report_sections = Vec::new();

    tx.send(Event::Thinking("Checking working tree status...".into()))?;
    if utils::has_uncommitted_changes(&tx, &target_path).await? {
        match run_commit_flow(&tx, &target_path).await? {
            CommitFlowResult::Committed(commit_msg) => report_sections.push(commit_msg),
            CommitFlowResult::Cancelled(reason) => {
                tx.send(Event::Answer(reason))?;
                return Ok(());
            }
        }
    }

    tx.send(Event::Thinking("Resolving release version...".into()))?;
    let Some((version, source)) =
        pick_release_version(&tx, &target_path, payload.version.as_deref()).await?
    else {
        tx.send(Event::Answer(
            "Release cancelled during crate selection.".to_string(),
        ))?;
        return Ok(());
    };

    let tag = normalize_tag(&version);

    tx.send(Event::Thinking(format!(
        "Pushing `{branch}` to `origin`..."
    )))?;
    let push_out = utils::execute_git_push(&tx, &target_path, "origin", branch).await?;
    report_sections.push(if push_out.is_empty() {
        format!("Pushed branch `{branch}` to `origin`.")
    } else {
        format!("Pushed branch `{branch}` to `origin`:\n\n```\n{push_out}\n```")
    });

    tx.send(Event::Thinking(format!(
        "Checking tag `{tag}` and pushing release..."
    )))?;

    if utils::git_tag_exists(&tx, &target_path, &tag).await? {
        info!(
            "Tag `{tag}` already exists in `{}`, skipping tag creation",
            target_path.display()
        );
        report_sections.push(format!(
            "Tag `{tag}` already exists (version `{version}` from {source}); skipped creating tag."
        ));
    } else {
        let tag_msg = format!("Release {tag}");
        let tag_status =
            utils::execute_git_tag(&tx, &target_path, &tag, Some(&tag_msg), Some("origin")).await?;
        report_sections.push(format!("{tag_status} (version `{version}` from {source})."));

        info!(
            "Released `{tag}` on `{branch}` in `{}`",
            target_path.display()
        );
    }

    tx.send(Event::Answer(report_sections.join("\n\n")))?;
    Ok(())
}

enum CommitFlowResult {
    Committed(String),
    Cancelled(String),
}

fn resolve_target_path(path: Option<String>, current_path: Option<PathBuf>) -> Result<PathBuf> {
    match path.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(p) => Ok(PathBuf::from(p)),
        None => current_path
            .ok_or(())
            .or_else(|_| std::env::current_dir().map_err(Into::into)),
    }
}

fn normalize_tag(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with('v') {
        trimmed.to_string()
    } else {
        format!("v{trimmed}")
    }
}

async fn run_commit_flow(tx: &Sender<Bytes>, target_path: &Path) -> Result<CommitFlowResult> {
    tx.send(Event::Thinking("Checking git status and changes...".into()))?;
    utils::ensure_git_repo(tx, target_path).await?;

    let mut stage_all = false;
    if utils::has_unstaged_changes(tx, target_path).await? {
        info!("ЗАПУСУКАЕМ ПОДТВЕРЖДЕНИЕ");
        let confirmation = Dialog::confirm(
            tx,
            "There are unstaged or untracked changes not added via `git add`. Include them in the commit?".to_string(),
            Some(Confirmation::No),
            Duration::from_secs(120),
        )
        .await?;

        match confirmation {
            Some(Confirmation::No) => {
                stage_all = false;
            }
            None => return Err(Error::OperationTimeout.into()),
            _ => {
                stage_all = true;
            }
        }
    }

    let diff = utils::get_git_diff(tx, target_path, !stage_all).await?;
    let cfg = Config::get();

    let selected_msg = loop {
        tx.send(Event::Thinking(
            "Generating commit message variants...".into(),
        ))?;

        let messages = Messages::new()
            .system(vec![cfg.llm.commit_prompt().into()])
            .user(vec![
                format!(
                    "Generate 2 distinct commit message variants for this diff:\n\n```diff\n{diff}\n```"
                )
                .into(),
            ])
            .wrap();

        #[derive(Deserialize, Schema)]
        struct CommitVariants {
            /// A concise single-line commit message summarizing the overall change (max 70-80 chars).
            pub short_message: String,
            /// A standard commit message with a single-line summary header, followed by a brief bulleted list detailing key changes.
            pub detailed_message: String,
        }

        let mut response = Completions::try_from(cfg.llm.diff_options())?
            .schema(CommitVariants::schema())
            .send(messages)
            .await?;

        let mut json_str = String::new();
        while let Some(chunk) = response.next().await {
            if let Chunk::Text(text) = chunk? {
                json_str.push_str(&text);
            }
        }

        let variants: CommitVariants = serde_json::from_str(&json_str)
            .map_err(|e| Error::Titled("Failed to parse commit variants JSON".into(), e.into()))?;

        let format_quote = |text: &str| format!("```text\n{text}\n```\n</br>");

        let select_options = vec![
            format!("Short:\n{}", format_quote(&variants.short_message)),
            format!("Detailed:\n{}", format_quote(&variants.detailed_message)),
            "🔄 Regenerate options...".to_string(),
        ];

        match Dialog::select(
            tx,
            "Choose a commit message variant:".to_string(),
            select_options,
            Duration::from_secs(120),
        )
        .await?
        {
            Some(Some(idx)) => {
                if idx == 2 {
                    continue;
                }

                let chosen = if idx == 0 {
                    variants.short_message
                } else {
                    variants.detailed_message
                };

                break chosen.trim().to_string();
            }
            Some(None) => {
                return Ok(CommitFlowResult::Cancelled(
                    "Commit operation was cancelled by the user.".to_string(),
                ));
            }
            None => return Err(Error::OperationTimeout.into()),
        }
    };

    let commit_output =
        utils::execute_git_commit(tx, target_path, &selected_msg, stage_all).await?;

    Ok(CommitFlowResult::Committed(format!(
        "Committed changes with message: `{}`\n\n```\n{}\n```",
        selected_msg, commit_output
    )))
}

async fn pick_release_version(
    tx: &Sender<Bytes>,
    target_path: &Path,
    explicit_version: Option<&str>,
) -> Result<Option<(String, String)>> {
    if let Some(ver) = explicit_version.map(str::trim).filter(|s| !s.is_empty()) {
        let clean_ver = ver.strip_prefix('v').unwrap_or(ver).to_string();
        return Ok(Some((clean_ver, "explicit parameter".to_string())));
    }

    match cargo::resolve_cargo_version(tx, target_path).await? {
        cargo::VersionResolution::Single(info) => {
            let source = format!("`{}` ({})", info.manifest_path.display(), info.name);
            Ok(Some((info.version, source)))
        }
        cargo::VersionResolution::Multiple(crates) => {
            let options: Vec<String> = crates
                .iter()
                .map(|c| {
                    format!(
                        "**{}** — `v{}` (`{}`)",
                        c.name,
                        c.version,
                        c.manifest_path.display()
                    )
                })
                .collect();

            match Dialog::select(
                tx,
                "Select crate to release version from:".to_string(),
                options,
                Duration::from_secs(120),
            )
            .await?
            {
                Some(Some(idx)) => {
                    let chosen = &crates[idx];
                    let source = format!("`{}` ({})", chosen.manifest_path.display(), chosen.name);
                    Ok(Some((chosen.version.clone(), source)))
                }
                Some(None) => Ok(None),
                None => Err(Error::OperationTimeout.into()),
            }
        }
        cargo::VersionResolution::NoneFound => Err(Error::Custom(
            "Could not determine release version from Cargo.toml (pass `version` explicitly)."
                .into(),
        )
        .into()),
    }
}
