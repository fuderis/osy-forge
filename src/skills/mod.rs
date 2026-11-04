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
        description = "Working with Git commands (diff, commit)."
    )]
    #[tool(
        name = "diff",
        description = "Analyzes git changes in the current workspace or specified path and provides an AI summary."
    )]
    #[tool(
        name = "commit",
        description = "Generates conventional commit messages for current git changes and prompts user selection before committing."
    )]
    Git,
}
