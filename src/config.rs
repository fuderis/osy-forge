use crate::prelude::*;

use anylm::options::Options;

const DIFF_PROMPT: &'static str = "\
You are an expert code reviewer and system developer. Your task is to analyze the provided `git diff` and write a clear, highly concise summary of changes.

Guidelines:
1. Focus on WHAT changed and WHY (business/technical logic), not line-by-line mechanical edits.
2. Group logical changes using clean bullet points.
3. Highlight any potential breaking changes, dangerous operations, or critical refactoring.
4. Keep the summary actionable, structured, and strictly concise (1-2 short paragraphs or clean bullet points).
5. Use markdown format (headings, lists, tables, etc.).\
";

const COMMIT_PROMPT: &str = "\
You are a Git commit expert following the Conventional Commits 1.0.0 specification (e.g., feat(scope): message, fix: message, refactor: message).

Analyze the provided `git diff` and generate EXACTLY 3 distinct commit message variants:
1. Variant 1 (Short): A concise single-line commit message summarizing the overall change (max 50-72 chars).
2. Variant 2 (Medium): A standard commit message with a single-line summary header, followed by a brief bulleted list detailing key changes.
3. Variant 3 (Detailed): A comprehensive commit message with a clear summary header, an in-depth bulleted breakdown of all technical modifications, and a note on the functional impact/benefit.

Requirements:
- Use imperative, present tense in headers and descriptions (\"add\", \"fix\", \"refactor\", not \"added\", \"fixed\").
- Keep formatting clean, precise, and git-ready.
- Do NOT wrap the generated messages in extra quotes or backticks.\
";

/// LLM completions/embeddings options.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LLMOptions {
    /// Git diff analysing prompt.
    pub diff_prompt: String,
    /// Git commit generation prompt.
    pub commit_prompt: String,

    /// Git diff analysing temperature.
    pub diff_temperature: f32,
    /// Git commit generation temperature.
    pub commit_temperature: f32,

    /// Completions provider options.
    pub completions: Options,
    /// Embeddings provider options.
    pub embeddings: Options,
}

impl Default for LLMOptions {
    fn default() -> Self {
        Self {
            diff_prompt: DIFF_PROMPT.trim().into(),
            commit_prompt: COMMIT_PROMPT.trim().into(),

            diff_temperature: 0.4,
            commit_temperature: 0.4,

            completions: Options::openai()
                .env_var("ROUTERAI_API_KEY")
                .base_url("https://routerai.ru/api")
                .model("qwen/qwen3-coder-next")
                .max_tokens(16384),
            embeddings: Options::openai()
                .env_var("ROUTERAI_API_KEY")
                .base_url("https://routerai.ru/api")
                .model("perplexity/pplx-embed-v1-0.6b"),
        }
    }
}

impl LLMOptions {
    pub fn diff_prompt(&self) -> &str {
        self.diff_prompt.trim()
    }
    pub fn commit_prompt(&self) -> &str {
        self.commit_prompt.trim()
    }

    pub fn diff_options(&self) -> Options {
        self.completions.clone().temperature(self.diff_temperature)
    }
    pub fn commit_options(&self) -> Options {
        self.completions
            .clone()
            .temperature(self.commit_temperature)
    }

    pub fn completions_options(&self) -> Options {
        self.completions.clone()
    }

    pub fn embeddings_options(&self) -> Options {
        self.embeddings.clone()
    }
}

/// System agent settings.
#[atoman::config]
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub llm: LLMOptions,
}
