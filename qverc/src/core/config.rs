//! Configuration parsing for qverc.toml

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    ReadError(#[from] std::io::Error),

    #[error("Failed to parse config: {0}")]
    ParseError(#[from] toml::de::Error),

    #[error("Failed to serialize config: {0}")]
    SerializeError(#[from] toml::ser::Error),
}

/// Gatekeeper verification configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatekeeperConfig {
    /// Tier 1 commands (syntax check, linter) - for Draft -> Valid
    #[serde(default)]
    pub tier1: Vec<String>,

    /// Tier 2 commands (unit tests) - for Valid -> Verified
    #[serde(default)]
    pub tier2: Vec<String>,

    /// Tier 3 commands (full integration) - for Verified -> Spine
    #[serde(default)]
    pub tier3: Vec<String>,
}

impl Default for GatekeeperConfig {
    fn default() -> Self {
        Self {
            tier1: Vec::new(),
            tier2: Vec::new(),
            tier3: Vec::new(),
        }
    }
}

/// Workspace configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    /// Patterns to ignore when scanning the workspace
    #[serde(default = "default_ignore")]
    pub ignore: Vec<String>,
}

fn default_ignore() -> Vec<String> {
    vec![
        ".qverc/".to_string(),
        "target/".to_string(),
        "node_modules/".to_string(),
        "*.log".to_string(),
        ".DS_Store".to_string(),
    ]
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            ignore: default_ignore(),
        }
    }
}

/// Plugin configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginConfig {
    /// Path to vector store plugin (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_store: Option<String>,
}

/// Main qverc configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Gatekeeper verification settings
    #[serde(default)]
    pub gatekeeper: GatekeeperConfig,

    /// Workspace settings
    #[serde(default)]
    pub workspace: WorkspaceConfig,

    /// Plugin settings
    #[serde(default)]
    pub plugins: PluginConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            gatekeeper: GatekeeperConfig::default(),
            workspace: WorkspaceConfig::default(),
            plugins: PluginConfig::default(),
        }
    }
}

impl Config {
    /// Load configuration from a file
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let content = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    /// Load configuration from the repository root
    pub fn load_from_repo(repo_root: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let config_path = repo_root.as_ref().join("qverc.toml");
        if config_path.exists() {
            Self::load(&config_path)
        } else {
            Ok(Self::default())
        }
    }

    /// Save configuration to a file
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), ConfigError> {
        let content = toml::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }

    /// Generate default configuration as TOML string
    pub fn default_toml() -> String {
        let config = Config {
            gatekeeper: GatekeeperConfig {
                tier1: vec!["echo 'No tier1 checks configured'".to_string()],
                tier2: vec!["echo 'No tier2 checks configured'".to_string()],
                tier3: vec!["echo 'No tier3 checks configured'".to_string()],
            },
            workspace: WorkspaceConfig::default(),
            plugins: PluginConfig::default(),
        };

        toml::to_string_pretty(&config).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(config.gatekeeper.tier1.is_empty());
        assert!(!config.workspace.ignore.is_empty());
    }

    #[test]
    fn test_config_roundtrip() {
        let config = Config::default();
        let toml_str = toml::to_string(&config).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(config.workspace.ignore.len(), parsed.workspace.ignore.len());
    }
}

