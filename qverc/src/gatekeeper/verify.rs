//! Tiered verification system
//!
//! The Gatekeeper runs configurable commands to verify code quality.

use crate::core::config::Config;
use crate::core::node::{Metrics, NodeStatus};
use std::path::Path;
use std::process::Command;
use std::time::Instant;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum GatekeeperError {
    #[error("Verification failed at tier {tier}: {message}")]
    VerificationFailed { tier: u8, message: String },

    #[error("Command execution error: {0}")]
    CommandError(#[from] std::io::Error),

    #[error("No commands configured for tier {0}")]
    NoCommandsConfigured(u8),
}

/// Verification tier
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Tier 1: Syntax check, linter
    Tier1,
    /// Tier 2: Unit tests
    Tier2,
    /// Tier 3: Full integration suite
    Tier3,
}

impl Tier {
    pub fn as_u8(&self) -> u8 {
        match self {
            Tier::Tier1 => 1,
            Tier::Tier2 => 2,
            Tier::Tier3 => 3,
        }
    }

    /// Get the target status after passing this tier
    pub fn target_status(&self) -> NodeStatus {
        match self {
            Tier::Tier1 => NodeStatus::Valid,
            Tier::Tier2 => NodeStatus::Verified,
            Tier::Tier3 => NodeStatus::Spine,
        }
    }
}

/// Result of a verification run
#[derive(Debug, Clone)]
pub struct VerificationResult {
    /// Whether verification passed
    pub passed: bool,

    /// The tier that was verified
    pub tier: Tier,

    /// Command outputs
    pub outputs: Vec<CommandOutput>,

    /// Metrics collected during verification
    pub metrics: Metrics,
}

/// Output from a single command
#[derive(Debug, Clone)]
pub struct CommandOutput {
    /// The command that was run
    pub command: String,

    /// Exit code (0 = success)
    pub exit_code: i32,

    /// Standard output
    pub stdout: String,

    /// Standard error
    pub stderr: String,

    /// Duration in milliseconds
    pub duration_ms: u64,
}

/// The Gatekeeper - runs verification commands
pub struct Gatekeeper {
    config: Config,
}

impl Gatekeeper {
    /// Create a new Gatekeeper with the given config
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Create a Gatekeeper with default (empty) config
    pub fn default_gatekeeper() -> Self {
        Self {
            config: Config::default(),
        }
    }

    /// Get commands for a tier
    fn get_commands(&self, tier: Tier) -> &[String] {
        match tier {
            Tier::Tier1 => &self.config.gatekeeper.tier1,
            Tier::Tier2 => &self.config.gatekeeper.tier2,
            Tier::Tier3 => &self.config.gatekeeper.tier3,
        }
    }

    /// Run verification at a specific tier
    pub fn verify(
        &self,
        tier: Tier,
        working_dir: impl AsRef<Path>,
    ) -> Result<VerificationResult, GatekeeperError> {
        let commands = self.get_commands(tier);

        // If no commands configured, pass by default
        if commands.is_empty() {
            return Ok(VerificationResult {
                passed: true,
                tier,
                outputs: Vec::new(),
                metrics: Metrics::default(),
            });
        }

        let working_dir = working_dir.as_ref();
        let mut outputs = Vec::new();
        let mut total_duration_ms = 0u64;
        let mut all_passed = true;

        for cmd_str in commands {
            let output = self.run_command(cmd_str, working_dir)?;
            total_duration_ms += output.duration_ms;

            if output.exit_code != 0 {
                all_passed = false;
            }

            outputs.push(output);

            // Stop on first failure
            if !all_passed {
                break;
            }
        }

        let metrics = Metrics {
            build_time_ms: Some(total_duration_ms),
            ..Default::default()
        };

        Ok(VerificationResult {
            passed: all_passed,
            tier,
            outputs,
            metrics,
        })
    }

    /// Run all tiers up to and including the specified tier
    pub fn verify_up_to(
        &self,
        max_tier: Tier,
        working_dir: impl AsRef<Path>,
    ) -> Result<VerificationResult, GatekeeperError> {
        let working_dir = working_dir.as_ref();
        let tiers = match max_tier {
            Tier::Tier1 => vec![Tier::Tier1],
            Tier::Tier2 => vec![Tier::Tier1, Tier::Tier2],
            Tier::Tier3 => vec![Tier::Tier1, Tier::Tier2, Tier::Tier3],
        };

        let mut all_outputs = Vec::new();
        let mut total_duration_ms = 0u64;

        for tier in tiers {
            let result = self.verify(tier, working_dir)?;
            total_duration_ms += result.metrics.build_time_ms.unwrap_or(0);
            all_outputs.extend(result.outputs);

            if !result.passed {
                return Ok(VerificationResult {
                    passed: false,
                    tier,
                    outputs: all_outputs,
                    metrics: Metrics {
                        build_time_ms: Some(total_duration_ms),
                        ..Default::default()
                    },
                });
            }
        }

        Ok(VerificationResult {
            passed: true,
            tier: max_tier,
            outputs: all_outputs,
            metrics: Metrics {
                build_time_ms: Some(total_duration_ms),
                ..Default::default()
            },
        })
    }

    /// Run a single command
    fn run_command(
        &self,
        cmd_str: &str,
        working_dir: &Path,
    ) -> Result<CommandOutput, GatekeeperError> {
        let start = Instant::now();

        // Use shell to run the command
        let output = if cfg!(target_os = "windows") {
            Command::new("cmd")
                .args(["/C", cmd_str])
                .current_dir(working_dir)
                .output()?
        } else {
            Command::new("sh")
                .args(["-c", cmd_str])
                .current_dir(working_dir)
                .output()?
        };

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(CommandOutput {
            command: cmd_str.to_string(),
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            duration_ms,
        })
    }

    /// Quick check if any commands are configured
    pub fn has_commands(&self, tier: Tier) -> bool {
        !self.get_commands(tier).is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_empty_config_passes() {
        let gatekeeper = Gatekeeper::default_gatekeeper();
        let temp_dir = TempDir::new().unwrap();

        let result = gatekeeper.verify(Tier::Tier1, temp_dir.path()).unwrap();
        assert!(result.passed);
        assert!(result.outputs.is_empty());
    }

    #[test]
    fn test_successful_command() {
        let mut config = Config::default();
        config.gatekeeper.tier1 = vec!["echo hello".to_string()];

        let gatekeeper = Gatekeeper::new(config);
        let temp_dir = TempDir::new().unwrap();

        let result = gatekeeper.verify(Tier::Tier1, temp_dir.path()).unwrap();
        assert!(result.passed);
        assert_eq!(result.outputs.len(), 1);
        assert_eq!(result.outputs[0].exit_code, 0);
        assert!(result.outputs[0].stdout.contains("hello"));
    }

    #[test]
    fn test_failing_command() {
        let mut config = Config::default();
        config.gatekeeper.tier1 = vec!["exit 1".to_string()];

        let gatekeeper = Gatekeeper::new(config);
        let temp_dir = TempDir::new().unwrap();

        let result = gatekeeper.verify(Tier::Tier1, temp_dir.path()).unwrap();
        assert!(!result.passed);
        assert_eq!(result.outputs[0].exit_code, 1);
    }

    #[test]
    fn test_tier_target_status() {
        assert_eq!(Tier::Tier1.target_status(), NodeStatus::Valid);
        assert_eq!(Tier::Tier2.target_status(), NodeStatus::Verified);
        assert_eq!(Tier::Tier3.target_status(), NodeStatus::Spine);
    }
}

