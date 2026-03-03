//! Skill system wiring: bootstrap + per-turn injection.

use std::path::PathBuf;

use bob_adapters::skills_agent::{
    RenderedSkillsPrompt, SkillPromptComposer, SkillSelectionPolicy, SkillSourceConfig,
};

use crate::config::SkillsConfig;

/// Build the skill prompt composer from configuration.
///
/// Returns `None` when skills are disabled or no sources are configured.
///
/// # Errors
///
/// Returns an error if skill loading from the configured sources fails.
pub fn build_skill_composer(cfg: &SkillsConfig) -> eyre::Result<Option<SkillPromptComposer>> {
    if !cfg.enabled || cfg.sources.is_empty() {
        return Ok(None);
    }

    let sources: Vec<SkillSourceConfig> = cfg
        .sources
        .iter()
        .map(|s| SkillSourceConfig { path: PathBuf::from(&s.path), recursive: s.recursive })
        .collect();

    let composer = SkillPromptComposer::from_sources(&sources, cfg.max_selected)?;
    tracing::info!(skill_count = composer.skills().len(), "skills loaded");
    Ok(Some(composer))
}

/// Render skill context for a given user input.
///
/// Returns the rendered prompt, selected skill names, and allowed tool list.
pub fn inject_skills_context(
    composer: &SkillPromptComposer,
    input: &str,
    token_budget: usize,
) -> RenderedSkillsPrompt {
    let policy = SkillSelectionPolicy {
        token_budget_tokens: token_budget,
        ..SkillSelectionPolicy::default()
    };
    composer.render_bundle_for_input_with_policy(input, &policy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{SkillSourceEntry, SkillsConfig};

    #[test]
    fn disabled_skills_returns_none() {
        let cfg = SkillsConfig { enabled: false, ..SkillsConfig::default() };
        let result = build_skill_composer(&cfg);
        assert!(result.is_ok());
        let Ok(result) = result else { return };
        assert!(result.is_none());
    }

    #[test]
    fn empty_sources_returns_none() {
        let cfg = SkillsConfig { enabled: true, sources: Vec::new(), ..SkillsConfig::default() };
        let result = build_skill_composer(&cfg);
        assert!(result.is_ok());
        let Ok(result) = result else { return };
        assert!(result.is_none());
    }

    #[test]
    fn nonexistent_source_returns_error() {
        let cfg = SkillsConfig {
            enabled: true,
            sources: vec![SkillSourceEntry {
                path: "/tmp/nonexistent-alice-skills-12345".to_string(),
                recursive: false,
            }],
            ..SkillsConfig::default()
        };
        let result = build_skill_composer(&cfg);
        assert!(result.is_err(), "nonexistent skill path should error");
    }

    #[test]
    fn build_composer_with_fixture_sources() {
        let fixtures =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/skills");
        let cfg = SkillsConfig {
            enabled: true,
            max_selected: 3,
            token_budget: 1800,
            sources: vec![SkillSourceEntry {
                path: fixtures.display().to_string(),
                recursive: true,
            }],
        };
        let result = build_skill_composer(&cfg);
        assert!(result.is_ok(), "fixture sources should load: {result:?}");
        let Ok(Some(composer)) = result else { return };
        assert_eq!(composer.skills().len(), 3, "should discover 3 fixture skills");
    }

    #[test]
    fn inject_context_returns_rendered_prompt() {
        let fixtures =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/skills");
        let cfg = SkillsConfig {
            enabled: true,
            max_selected: 3,
            token_budget: 1800,
            sources: vec![SkillSourceEntry {
                path: fixtures.display().to_string(),
                recursive: true,
            }],
        };
        let Ok(Some(composer)) = build_skill_composer(&cfg) else { return };

        let bundle = inject_skills_context(&composer, "write rust tests", 1800);
        // Should select at least one skill for Rust testing input
        assert!(
            !bundle.selected_skill_names.is_empty(),
            "should select at least one skill for 'write rust tests'"
        );
        assert!(bundle.selected_skill_names.len() <= 3, "should not exceed max_selected");
        // Prompt should have been rendered with content
        assert!(!bundle.prompt.is_empty(), "rendered prompt should not be empty");
    }
}
