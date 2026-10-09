pub mod git;

use crate::prelude::*;

use anylm::api::Tool;
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
    #[tools("diff", "commit")]
    Git,
}
