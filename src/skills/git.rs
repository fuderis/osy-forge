use crate::{prelude::*, utils};

use anylm::{
    IntoSchema, Messages, Schema,
    completions::{Chunk, Completions},
};

// ============================================================================
// DTO STRUCTS
// ============================================================================

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

// ============================================================================
// HANDLERS
// ============================================================================

#[log(path = %query.payload.path.as_deref().unwrap_or("default"))]
pub async fn handle_diff(tx: Sender<Bytes>, query: ToolQuery<DiffAction>) -> Result<()> {
    let ToolQuery {
        current_path,
        payload,
    } = query;

    let target_path = match payload.path.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(p) => PathBuf::from(p),
        None => current_path.unwrap_or(std::env::current_dir()?),
    };

    tx.send(Event::Thinking(
        "Checking git repository and diff...".into(),
    ))?;
    let diff = utils::get_git_diff(&target_path).await?;

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

    let target_path = match payload.path.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(p) => PathBuf::from(p),
        None => current_path.unwrap_or(std::env::current_dir()?),
    };

    tx.send(Event::Thinking("Checking git status and changes...".into()))?;
    let diff = utils::get_git_diff(&target_path).await?;

    tx.send(Event::Thinking(
        "Generating commit message variants...".into(),
    ))?;

    let cfg = Config::get();
    let messages = Messages::new()
        .system(vec![cfg.llm.commit_prompt().into()])
        .user(vec![
            format!("Generate 3 commit message variants for this diff:\n\n```diff\n{diff}\n```")
                .into(),
        ])
        .wrap();

    #[derive(Deserialize, Schema)]
    struct CommitVariants {
        /// List of exactly 3 commit message options following Conventional Commits format.
        pub options: Vec<String>,
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

    if variants.options.is_empty() {
        return Err(Error::Custom(str!("LLM failed to generate commit variants.")).into());
    }

    let selected_msg = match Dialog::select(
        &tx,
        "Choose a commit message:".to_string(),
        variants.options.iter().map(|s| format!("{s}\n")).collect(),
        Duration::from_secs(120),
    )
    .await?
    {
        Some(Some(idx)) => variants
            .options
            .get(idx)
            .expect("Selected index is out of bounds.")
            .trim()
            .to_string(),
        Some(None) => {
            tx.send(Event::Answer(
                "Commit operation was cancelled by the user.".to_string(),
            ))?;
            return Ok(());
        }
        None => {
            return Err(Error::OperationTimeout.into());
        }
    };

    let commit_output = utils::execute_git_commit(&target_path, &selected_msg).await?;

    let msg = format!(
        "Committed changes with message: `{}`\n\n```\n{}\n```",
        selected_msg, commit_output
    );

    info!("Git commit executed in `{}`", target_path.display());
    tx.send(Event::Answer(msg))?;
    Ok(())
}
