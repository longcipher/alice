//! Alice runtime configuration.

use std::collections::HashMap;

use serde::Deserialize;

/// Top-level Alice configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct AliceConfig {
    /// Runtime behavior settings.
    pub runtime: RuntimeConfig,
    /// Memory subsystem settings.
    #[serde(default)]
    pub memory: MemoryConfig,
    /// Skill system settings.
    #[serde(default)]
    pub skills: SkillsConfig,
    /// Channel settings.
    #[serde(default)]
    pub channels: ChannelsConfig,
    /// Optional MCP tool server configuration.
    #[serde(default)]
    pub mcp: McpConfig,
}

/// Runtime settings for Bob orchestration.
#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeConfig {
    /// Default model identifier.
    pub default_model: String,
    /// Maximum turn steps.
    pub max_steps: Option<u32>,
    /// Turn timeout in milliseconds.
    pub turn_timeout_ms: Option<u64>,
    /// Dispatch mode preference.
    pub dispatch_mode: Option<DispatchMode>,
}

/// Dispatch mode used by Bob runtime.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DispatchMode {
    /// Prompt-guided action parsing only.
    PromptGuided,
    /// Native tool-calling first, fallback to prompt-guided.
    NativePreferred,
}

/// Memory subsystem settings.
#[derive(Debug, Clone, Deserialize)]
pub struct MemoryConfig {
    /// SQLite file path.
    pub db_path: String,
    /// Max recalled memories per turn.
    pub recall_limit: usize,
    /// BM25 weight in score fusion.
    pub bm25_weight: f32,
    /// Vector weight in score fusion.
    pub vector_weight: f32,
    /// Vector dimension length.
    pub vector_dimensions: usize,
    /// Enable vector storage and retrieval.
    pub enable_vector: bool,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            db_path: ".alice/memory.db".to_string(),
            recall_limit: 6,
            bm25_weight: 0.3,
            vector_weight: 0.7,
            vector_dimensions: 384,
            enable_vector: true,
        }
    }
}

const fn default_true() -> bool {
    true
}

const fn default_max_selected() -> usize {
    3
}

const fn default_token_budget() -> usize {
    1800
}

/// Skill system settings.
#[derive(Debug, Clone, Deserialize)]
pub struct SkillsConfig {
    /// Whether skill system is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Maximum skills to select per turn.
    #[serde(default = "default_max_selected")]
    pub max_selected: usize,
    /// Token budget for skill prompt injection.
    #[serde(default = "default_token_budget")]
    pub token_budget: usize,
    /// Skill source directories.
    #[serde(default)]
    pub sources: Vec<SkillSourceEntry>,
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self { enabled: true, max_selected: 3, token_budget: 1800, sources: Vec::new() }
    }
}

/// A skill source directory entry.
#[derive(Debug, Clone, Deserialize)]
pub struct SkillSourceEntry {
    /// Path to the skill directory.
    pub path: String,
    /// Whether to search subdirectories recursively.
    #[serde(default)]
    pub recursive: bool,
}

/// Channel provider settings.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ChannelsConfig {
    /// Discord channel settings.
    #[serde(default)]
    pub discord: ChannelProviderConfig,
    /// Telegram channel settings.
    #[serde(default)]
    pub telegram: ChannelProviderConfig,
}

/// Individual channel provider configuration.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ChannelProviderConfig {
    /// Whether this channel is enabled.
    #[serde(default)]
    pub enabled: bool,
}

/// MCP server list.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct McpConfig {
    /// Configured MCP servers.
    #[serde(default)]
    pub servers: Vec<McpServerConfig>,
}

/// One MCP server entry.
#[derive(Debug, Clone, Deserialize)]
pub struct McpServerConfig {
    /// Namespace id for the server.
    pub id: String,
    /// Command executable.
    pub command: String,
    /// Command arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Additional environment variables.
    pub env: Option<HashMap<String, String>>,
    /// Optional timeout override for this server.
    pub tool_timeout_ms: Option<u64>,
}

/// Load Alice config from a TOML file path.
///
/// # Errors
///
/// Returns an error if the file cannot be read or parsed.
pub fn load_config(path: &str) -> eyre::Result<AliceConfig> {
    let settings = config::Config::builder()
        .add_source(config::File::with_name(path).required(true))
        .build()?;
    let config: AliceConfig = settings.try_deserialize()?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_config() {
        let input = r#"
[runtime]
default_model = "openai:gpt-4o-mini"
"#;

        let config = config::Config::builder()
            .add_source(config::File::from_str(input, config::FileFormat::Toml))
            .build();
        assert!(config.is_ok(), "minimal config should parse");
        let Ok(config) = config else { return };

        let parsed: Result<AliceConfig, config::ConfigError> = config.try_deserialize();
        assert!(parsed.is_ok(), "minimal config should deserialize");
        let Ok(parsed) = parsed else { return };

        assert_eq!(parsed.runtime.default_model, "openai:gpt-4o-mini");
        assert_eq!(parsed.memory.recall_limit, 6);
        assert!(parsed.mcp.servers.is_empty());
        // skills defaults
        assert!(parsed.skills.enabled);
        assert_eq!(parsed.skills.max_selected, 3);
        assert_eq!(parsed.skills.token_budget, 1800);
        assert!(parsed.skills.sources.is_empty());
        // channels defaults
        assert!(!parsed.channels.discord.enabled);
        assert!(!parsed.channels.telegram.enabled);
    }

    #[test]
    fn parse_full_config() {
        let input = r#"
[runtime]
default_model = "openai:gpt-4o-mini"
max_steps = 9
turn_timeout_ms = 55000
dispatch_mode = "prompt_guided"

[memory]
db_path = "./tmp/alice.db"
recall_limit = 4
bm25_weight = 0.4
vector_weight = 0.6
vector_dimensions = 256
enable_vector = false

[skills]
enabled = false
max_selected = 5
token_budget = 2000

[[skills.sources]]
path = ".alice/skills"
recursive = true

[channels.discord]
enabled = true

[channels.telegram]
enabled = true

[[mcp.servers]]
id = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "."]
tool_timeout_ms = 15000
"#;

        let config = config::Config::builder()
            .add_source(config::File::from_str(input, config::FileFormat::Toml))
            .build();
        assert!(config.is_ok(), "full config should parse");
        let Ok(config) = config else { return };

        let parsed: Result<AliceConfig, config::ConfigError> = config.try_deserialize();
        assert!(parsed.is_ok(), "full config should deserialize");
        let Ok(parsed) = parsed else { return };

        assert_eq!(parsed.runtime.max_steps, Some(9));
        assert_eq!(parsed.runtime.dispatch_mode, Some(DispatchMode::PromptGuided));
        assert_eq!(parsed.memory.vector_dimensions, 256);
        assert!(!parsed.memory.enable_vector);
        assert_eq!(parsed.mcp.servers.len(), 1);
        assert_eq!(parsed.mcp.servers[0].id, "filesystem");
        // skills
        assert!(!parsed.skills.enabled);
        assert_eq!(parsed.skills.max_selected, 5);
        assert_eq!(parsed.skills.token_budget, 2000);
        assert_eq!(parsed.skills.sources.len(), 1);
        assert_eq!(parsed.skills.sources[0].path, ".alice/skills");
        assert!(parsed.skills.sources[0].recursive);
        // channels
        assert!(parsed.channels.discord.enabled);
        assert!(parsed.channels.telegram.enabled);
    }

    #[test]
    fn parse_config_native_preferred_dispatch() {
        let input = r#"
[runtime]
default_model = "openai:gpt-4o-mini"
dispatch_mode = "native_preferred"
"#;

        let config = config::Config::builder()
            .add_source(config::File::from_str(input, config::FileFormat::Toml))
            .build();
        assert!(config.is_ok(), "native_preferred config should parse");
        let Ok(config) = config else { return };

        let parsed: Result<AliceConfig, config::ConfigError> = config.try_deserialize();
        assert!(parsed.is_ok(), "native_preferred config should deserialize");
        let Ok(parsed) = parsed else { return };

        assert_eq!(parsed.runtime.dispatch_mode, Some(DispatchMode::NativePreferred));
        // Omitted optional fields should be None
        assert!(parsed.runtime.max_steps.is_none());
        assert!(parsed.runtime.turn_timeout_ms.is_none());
    }

    #[test]
    fn parse_config_multiple_skill_sources() {
        let input = r#"
[runtime]
default_model = "openai:gpt-4o-mini"

[skills]
enabled = true
max_selected = 2
token_budget = 1200

[[skills.sources]]
path = ".alice/skills"
recursive = true

[[skills.sources]]
path = "/opt/shared-skills"
recursive = false

[[skills.sources]]
path = "~/custom-skills"
"#;

        let config = config::Config::builder()
            .add_source(config::File::from_str(input, config::FileFormat::Toml))
            .build();
        assert!(config.is_ok(), "multi-source config should parse");
        let Ok(config) = config else { return };

        let parsed: Result<AliceConfig, config::ConfigError> = config.try_deserialize();
        assert!(parsed.is_ok(), "multi-source config should deserialize");
        let Ok(parsed) = parsed else { return };

        assert_eq!(parsed.skills.sources.len(), 3);
        assert!(parsed.skills.sources[0].recursive);
        assert!(!parsed.skills.sources[1].recursive);
        // Third source omits recursive, should default to false
        assert!(!parsed.skills.sources[2].recursive);
        assert_eq!(parsed.skills.sources[1].path, "/opt/shared-skills");
        assert_eq!(parsed.skills.max_selected, 2);
        assert_eq!(parsed.skills.token_budget, 1200);
    }

    #[test]
    fn parse_config_multiple_mcp_servers_with_env() {
        let input = r#"
[runtime]
default_model = "openai:gpt-4o-mini"

[[mcp.servers]]
id = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "."]

[[mcp.servers]]
id = "github"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]
tool_timeout_ms = 30000

[mcp.servers.env]
GITHUB_TOKEN = "test-token"
"#;

        let config = config::Config::builder()
            .add_source(config::File::from_str(input, config::FileFormat::Toml))
            .build();
        assert!(config.is_ok(), "multi-mcp config should parse");
        let Ok(config) = config else { return };

        let parsed: Result<AliceConfig, config::ConfigError> = config.try_deserialize();
        assert!(parsed.is_ok(), "multi-mcp config should deserialize");
        let Ok(parsed) = parsed else { return };

        assert_eq!(parsed.mcp.servers.len(), 2);
        // First server has no env or timeout override
        assert_eq!(parsed.mcp.servers[0].id, "filesystem");
        assert!(parsed.mcp.servers[0].env.is_none());
        assert!(parsed.mcp.servers[0].tool_timeout_ms.is_none());
        // Second server has env + timeout
        assert_eq!(parsed.mcp.servers[1].id, "github");
        assert_eq!(parsed.mcp.servers[1].tool_timeout_ms, Some(30_000));
        let Some(ref env) = parsed.mcp.servers[1].env else { return };
        assert_eq!(env.get("GITHUB_TOKEN").map(String::as_str), Some("test-token"));
    }
}
