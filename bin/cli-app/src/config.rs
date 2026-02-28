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
        let Ok(config) = config else {
            return;
        };

        let parsed: Result<AliceConfig, config::ConfigError> = config.try_deserialize();
        assert!(parsed.is_ok(), "minimal config should deserialize");
        let Ok(parsed) = parsed else {
            return;
        };

        assert_eq!(parsed.runtime.default_model, "openai:gpt-4o-mini");
        assert_eq!(parsed.memory.recall_limit, 6);
        assert!(parsed.mcp.servers.is_empty());
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
        let Ok(config) = config else {
            return;
        };

        let parsed: Result<AliceConfig, config::ConfigError> = config.try_deserialize();
        assert!(parsed.is_ok(), "full config should deserialize");
        let Ok(parsed) = parsed else {
            return;
        };

        assert_eq!(parsed.runtime.max_steps, Some(9));
        assert_eq!(parsed.runtime.dispatch_mode, Some(DispatchMode::PromptGuided));
        assert_eq!(parsed.memory.vector_dimensions, 256);
        assert!(!parsed.memory.enable_vector);
        assert_eq!(parsed.mcp.servers.len(), 1);
        assert_eq!(parsed.mcp.servers[0].id, "filesystem");
    }
}
