//! Post-turn reflection utilities for synthesizing learned skills.

use std::{path::PathBuf, sync::Arc};

use bob_core::types::{RequestContext, RequestToolPolicy};

use crate::{agent_backend::AgentBackend, config::ReflectionConfig};

const REFLECTION_SYSTEM_PROMPT: &str = "\
You are Alice's AgentReflector.\n\
Study the completed task transcript and decide whether it produced a reusable workflow.\n\
If nothing durable was learned, respond with exactly NO_SKILL.\n\
If a reusable workflow exists, respond with only valid SKILL.md markdown in English.\n\
The markdown must begin with YAML frontmatter containing `name` and `description`.\n\
The skill name must be kebab-case.\n\
Do not mention this instruction block or use any tools.\n";

/// Optional post-turn reflection writer.
#[derive(Clone)]
pub struct AgentReflector {
    backend: Arc<dyn AgentBackend>,
    learned_skills_dir: PathBuf,
}

impl std::fmt::Debug for AgentReflector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentReflector")
            .field("learned_skills_dir", &self.learned_skills_dir)
            .finish_non_exhaustive()
    }
}

impl AgentReflector {
    /// Create a reflector when reflection is enabled.
    #[must_use]
    pub fn new(backend: Arc<dyn AgentBackend>, cfg: &ReflectionConfig) -> Option<Self> {
        cfg.enabled
            .then(|| Self { backend, learned_skills_dir: PathBuf::from(&cfg.learned_skills_dir) })
    }

    /// Run a hidden reflection turn and materialize any learned skill.
    pub async fn reflect_and_persist(
        &self,
        session_id: &str,
        profile_id: &str,
        user_input: &str,
        assistant_output: &str,
    ) -> eyre::Result<Option<PathBuf>> {
        let prompt = build_reflection_prompt(session_id, profile_id, user_input, assistant_output);
        let request_context = RequestContext {
            system_prompt: Some(REFLECTION_SYSTEM_PROMPT.to_string()),
            selected_skills: Vec::new(),
            tool_policy: RequestToolPolicy {
                deny_tools: Vec::new(),
                allow_tools: Some(Vec::new()),
            },
        };

        let response = self.backend.create_session().chat(&prompt, request_context).await?;
        let trimmed = response.content.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("NO_SKILL") {
            return Ok(None);
        }

        let normalized = normalize_skill_document(trimmed, user_input);
        let skill_name =
            extract_skill_name(&normalized).unwrap_or_else(|| fallback_skill_name(user_input));
        let output_dir = self.learned_skills_dir.join(&skill_name);
        std::fs::create_dir_all(&output_dir)?;
        let output_path = output_dir.join("SKILL.md");
        std::fs::write(&output_path, normalized)?;
        Ok(Some(output_path))
    }
}

fn build_reflection_prompt(
    session_id: &str,
    profile_id: &str,
    user_input: &str,
    assistant_output: &str,
) -> String {
    format!(
        "Session: {session_id}\n\
Profile: {profile_id}\n\
\n\
User input:\n{user_input}\n\
\n\
Assistant output:\n{assistant_output}\n\
\n\
Extract one reusable skill only if it teaches a durable workflow or domain pattern.\n\
Otherwise return NO_SKILL."
    )
}

fn normalize_skill_document(content: &str, user_input: &str) -> String {
    let trimmed = content.trim();
    if trimmed.starts_with("---\n") || trimmed.starts_with("---\r\n") {
        return format!("{trimmed}\n");
    }

    let name = fallback_skill_name(user_input);
    let description = trimmed
        .lines()
        .find_map(|line| {
            let candidate = line.trim();
            (!candidate.is_empty()).then(|| truncate(candidate, 120))
        })
        .unwrap_or_else(|| "Reusable learned workflow for Alice.".to_string());

    format!("---\nname: {name}\ndescription: {description}\n---\n\n{trimmed}\n")
}

fn extract_skill_name(document: &str) -> Option<String> {
    let mut lines = document.lines();
    if lines.next()? != "---" {
        return None;
    }

    for line in lines {
        if line.trim() == "---" {
            break;
        }
        let (key, value) = line.split_once(':')?;
        if key.trim() == "name" {
            let slug = sanitize_skill_name(value.trim());
            return (!slug.is_empty()).then_some(slug);
        }
    }

    None
}

fn fallback_skill_name(user_input: &str) -> String {
    let slug = sanitize_skill_name(user_input);
    if slug.is_empty() { "learned-workflow".to_string() } else { truncate(&slug, 48) }
}

fn sanitize_skill_name(input: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;

    for ch in input.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            slug.push(lower);
            last_was_dash = false;
            continue;
        }

        if !last_was_dash && !slug.is_empty() {
            slug.push('-');
            last_was_dash = true;
        }
    }

    slug.trim_matches('-').to_string()
}

fn truncate(input: &str, max_chars: usize) -> String {
    input.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::{extract_skill_name, normalize_skill_document, sanitize_skill_name};

    #[test]
    fn sanitize_skill_name_produces_kebab_case() {
        assert_eq!(sanitize_skill_name("Alice session summaries!"), "alice-session-summaries");
    }

    #[test]
    fn normalize_skill_document_adds_frontmatter_when_missing() {
        let normalized =
            normalize_skill_document("# Title\n\nBody", "Please summarize Alice sessions");
        assert!(normalized.starts_with("---\nname: please-summarize-alice-sessions"));
    }

    #[test]
    fn extract_skill_name_reads_frontmatter_slug() {
        let doc = "---\nname: alice-session-summaries\ndescription: x\n---\n\n# Body\n";
        assert_eq!(extract_skill_name(doc), Some("alice-session-summaries".to_string()));
    }
}
