pub mod git;

use crate::prelude::*;
use osy_skill::AgentSkill;

/// System agent skills.
#[derive(AgentSkill, Clone, Copy, Debug, Display, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
#[display(rename_all = "snake_case")]
pub enum SkillKind {
    #[skill(
        module = "git",
        description = "Working with Git commands (diff, commit, push, tag, release)."
    )]
    #[tool(
        name = "diff",
        description = "Analyzes git changes in the current workspace or specified path and provides an AI summary."
    )]
    #[tool(
        name = "commit",
        description = "Generates conventional commit messages for current git changes and prompts user selection before committing."
    )]
    #[tool(
        name = "push",
        description = "Pushes the current HEAD to the target branch (defaults to `main`) on the remote (defaults to `origin`)."
    )]
    #[tool(
        name = "tag",
        description = "Creates a version git tag (from argument or Cargo.toml) and optionally pushes it to remote."
    )]
    #[tool(
        name = "release",
        description = "Commits pending changes if needed, resolves version from Cargo.toml or workspace crate selection, pushes branch, and creates/pushes a version tag."
    )]
    Git,
}
