//! State Node and Manifest definitions
//!
//! The atomic unit of qverc - a complete, self-contained system state.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Node status in the verification pipeline
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeStatus {
    /// Initial state, not yet verified
    Draft,
    /// Passed Tier 1 verification (syntax/linter)
    Valid,
    /// Passed Tier 2 verification (unit tests)
    Verified,
    /// On the spine, passed full verification
    Spine,
}

impl NodeStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            NodeStatus::Draft => "draft",
            NodeStatus::Valid => "valid",
            NodeStatus::Verified => "verified",
            NodeStatus::Spine => "spine",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "draft" => Some(NodeStatus::Draft),
            "valid" => Some(NodeStatus::Valid),
            "verified" => Some(NodeStatus::Verified),
            "spine" => Some(NodeStatus::Spine),
            _ => None,
        }
    }
}

impl std::fmt::Display for NodeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Zone in the DAG topology
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Zone {
    /// The Exploration Zone - ephemeral, superposition branches
    Exploration,
    /// The Consolidation Zone - the spine, permanent nodes
    Consolidation,
}

impl Zone {
    pub fn as_str(&self) -> &'static str {
        match self {
            Zone::Exploration => "exploration",
            Zone::Consolidation => "consolidation",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "exploration" => Some(Zone::Exploration),
            "consolidation" => Some(Zone::Consolidation),
            _ => None,
        }
    }
}

impl std::fmt::Display for Zone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Performance metrics captured during verification
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Metrics {
    /// Build time in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_time_ms: Option<u64>,

    /// Runtime latency P99 in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_latency_p99: Option<u64>,

    /// Binary size in kilobytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binary_size_kb: Option<u64>,

    /// Test duration in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_duration_ms: Option<u64>,

    /// Number of tests passed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tests_passed: Option<u32>,

    /// Number of tests failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tests_failed: Option<u32>,
}

/// A failed verification attempt - negative knowledge for future agents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureRecord {
    /// Hash of the failed attempt
    pub attempt_hash: String,

    /// Error message
    pub error: String,

    /// Tier at which it failed
    pub tier: u8,

    /// Timestamp of failure
    pub timestamp: DateTime<Utc>,
}

/// The semantic manifest carried by each node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// Unique node identifier (e.g., "qv-8a9b2c")
    pub node_id: String,

    /// Parent node IDs (can have multiple for merges)
    pub parents: Vec<String>,

    /// The intent/prompt that generated this state
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent_prompt: Option<String>,

    /// Agent model signature (e.g., "gpt-4-turbo-v2")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_signature: Option<String>,

    /// Current verification status
    pub status: NodeStatus,

    /// Zone in the DAG
    pub zone: Zone,

    /// Performance metrics
    #[serde(default, skip_serializing_if = "Metrics::is_empty")]
    pub metrics: Metrics,

    /// History of failed attempts (negative knowledge)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failure_history: Vec<FailureRecord>,

    /// Creation timestamp
    pub created_at: DateTime<Utc>,

    /// BLAKE3 hash of the file tree
    pub tree_hash: String,
}

impl Metrics {
    pub fn is_empty(&self) -> bool {
        self.build_time_ms.is_none()
            && self.runtime_latency_p99.is_none()
            && self.binary_size_kb.is_none()
            && self.test_duration_ms.is_none()
            && self.tests_passed.is_none()
            && self.tests_failed.is_none()
    }
}

/// A file entry in a node's tree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    /// Relative path from repository root
    pub path: String,

    /// BLAKE3 hash of file contents
    pub blob_hash: String,

    /// Unix file mode (permissions)
    pub mode: u32,
}

/// A complete State Node
#[derive(Debug, Clone)]
pub struct Node {
    /// The semantic manifest
    pub manifest: Manifest,

    /// Files in this state
    pub files: Vec<FileEntry>,
}

impl Node {
    /// Create a new node
    pub fn new(
        node_id: String,
        parents: Vec<String>,
        tree_hash: String,
        files: Vec<FileEntry>,
    ) -> Self {
        Self {
            manifest: Manifest {
                node_id,
                parents,
                intent_prompt: None,
                agent_signature: None,
                status: NodeStatus::Draft,
                zone: Zone::Exploration,
                metrics: Metrics::default(),
                failure_history: Vec::new(),
                created_at: Utc::now(),
                tree_hash,
            },
            files,
        }
    }

    /// Set the intent prompt
    pub fn with_intent(mut self, intent: impl Into<String>) -> Self {
        let intent = intent.into();
        if !intent.is_empty() {
            self.manifest.intent_prompt = Some(intent);
        }
        self
    }

    /// Set the agent signature
    pub fn with_agent(mut self, agent: impl Into<String>) -> Self {
        let agent = agent.into();
        if !agent.is_empty() {
            self.manifest.agent_signature = Some(agent);
        }
        self
    }

    /// Update the status
    pub fn with_status(mut self, status: NodeStatus) -> Self {
        self.manifest.status = status;
        self
    }

    /// Update the zone
    pub fn with_zone(mut self, zone: Zone) -> Self {
        self.manifest.zone = zone;
        self
    }

    /// Get the node ID
    pub fn id(&self) -> &str {
        &self.manifest.node_id
    }

    /// Get parent IDs
    pub fn parents(&self) -> &[String] {
        &self.manifest.parents
    }
}

/// Generate a short node ID from components
pub fn generate_node_id(parents: &[String], tree_hash: &str, timestamp: DateTime<Utc>) -> String {
    use blake3::Hasher;

    let mut hasher = Hasher::new();

    // Hash parents
    for parent in parents {
        hasher.update(parent.as_bytes());
    }

    // Hash tree
    hasher.update(tree_hash.as_bytes());

    // Hash timestamp
    hasher.update(&timestamp.timestamp_nanos_opt().unwrap_or(0).to_le_bytes());

    let hash = hasher.finalize();
    let hex = hash.to_hex();

    // Take first 6 characters for short ID
    format!("qv-{}", &hex[..6])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_id_generation() {
        let parents = vec!["qv-abc123".to_string()];
        let tree_hash = "deadbeef".to_string();
        let timestamp = Utc::now();

        let id = generate_node_id(&parents, &tree_hash, timestamp);
        assert!(id.starts_with("qv-"));
        assert_eq!(id.len(), 9); // "qv-" + 6 hex chars
    }

    #[test]
    fn test_node_status_roundtrip() {
        for status in [
            NodeStatus::Draft,
            NodeStatus::Valid,
            NodeStatus::Verified,
            NodeStatus::Spine,
        ] {
            assert_eq!(NodeStatus::from_str(status.as_str()), Some(status));
        }
    }
}

