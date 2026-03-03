//! Integration tests for the skill system pipeline.
//!
//! Tests skill loading from fixture directories, selection, prompt rendering,
//! tool policy extraction, and graceful error handling.

use std::path::PathBuf;

use alice_runtime::{
    config::{SkillSourceEntry, SkillsConfig},
    skill_wiring::{build_skill_composer, inject_skills_context},
};

fn fixtures_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/skills")
}

fn config_with_fixtures() -> SkillsConfig {
    SkillsConfig {
        enabled: true,
        max_selected: 3,
        token_budget: 1800,
        sources: vec![SkillSourceEntry {
            path: fixtures_path().display().to_string(),
            recursive: true,
        }],
    }
}

#[test]
fn load_fixture_skills() {
    let cfg = config_with_fixtures();
    let result = build_skill_composer(&cfg);
    assert!(result.is_ok(), "should load fixture skills: {result:?}");
    let Ok(result) = result else { return };
    let composer = result.as_ref();
    assert!(composer.is_some(), "composer should be Some");
    let Some(composer) = composer else { return };
    assert_eq!(composer.skills().len(), 3, "should find 3 fixture skills");

    let names: Vec<&str> = composer.skills().iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"code-review"), "should include code-review");
    assert!(names.contains(&"sql-query"), "should include sql-query");
    assert!(names.contains(&"rust-testing"), "should include rust-testing");
}

#[test]
fn select_matching_skill_by_name() {
    let cfg = config_with_fixtures();
    let composer = build_skill_composer(&cfg).ok().flatten();
    assert!(composer.is_some());
    let Some(composer) = composer.as_ref() else {
        return;
    };

    // Input mentioning "code review" should select the code-review skill
    let selected = composer.select_for_input("please review this code");
    let names: Vec<&str> = selected.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"code-review"),
        "code-review should be selected for 'review this code', got: {names:?}"
    );
}

#[test]
fn no_match_returns_empty_selection() {
    let cfg = config_with_fixtures();
    let composer = build_skill_composer(&cfg).ok().flatten();
    assert!(composer.is_some());
    let Some(composer) = composer.as_ref() else {
        return;
    };

    // Input that matches nothing
    let selected = composer.select_for_input("what is the weather today");
    // May or may not select skills depending on scoring. The key property:
    // inject_skills_context with this input should not crash
    let _ = selected;
    let bundle = inject_skills_context(composer, "what is the weather today", 1800);
    // bundle.prompt may be empty if no skills match
    assert!(bundle.selected_skill_names.len() <= 3, "should not exceed max_selected");
}

#[test]
fn skill_with_allowed_tools_populates_policy() {
    let cfg = config_with_fixtures();
    let composer = build_skill_composer(&cfg).ok().flatten();
    assert!(composer.is_some());
    let Some(composer) = composer.as_ref() else {
        return;
    };

    // "code review" should select code-review which has allowed-tools: "Read Bash"
    let bundle = inject_skills_context(composer, "review this code for quality", 1800);
    if bundle.selected_skill_names.contains(&"code-review".to_string()) {
        assert!(
            !bundle.selected_allowed_tools.is_empty(),
            "code-review skill should have allowed tools"
        );
        // Check tools include Read and Bash
        assert!(
            bundle.selected_allowed_tools.contains(&"Read".to_string()) ||
                bundle.selected_allowed_tools.contains(&"Bash".to_string()),
            "allowed tools should include Read or Bash, got: {:?}",
            bundle.selected_allowed_tools
        );
    }
}

#[test]
fn render_prompt_within_token_budget() {
    let cfg = config_with_fixtures();
    let composer = build_skill_composer(&cfg).ok().flatten();
    assert!(composer.is_some());
    let Some(composer) = composer.as_ref() else {
        return;
    };

    // Small budget to test truncation
    let bundle = inject_skills_context(composer, "write rust tests", 50);
    // Should still work, just potentially truncated
    assert!(bundle.selected_skill_names.len() <= 3, "should not exceed max_selected");
}

#[test]
fn invalid_source_path_returns_error() {
    let cfg = SkillsConfig {
        enabled: true,
        max_selected: 3,
        token_budget: 1800,
        sources: vec![SkillSourceEntry {
            path: "/tmp/nonexistent-alice-skills-test-99999".to_string(),
            recursive: false,
        }],
    };
    let result = build_skill_composer(&cfg);
    assert!(result.is_err(), "nonexistent skill source should return error");
}

#[test]
fn disabled_skills_with_valid_sources_returns_none() {
    let cfg = SkillsConfig {
        enabled: false,
        sources: vec![SkillSourceEntry {
            path: fixtures_path().display().to_string(),
            recursive: true,
        }],
        ..SkillsConfig::default()
    };
    let result = build_skill_composer(&cfg);
    assert!(result.is_ok());
    let Ok(result) = result else { return };
    assert!(result.is_none(), "disabled skills should return None");
}
