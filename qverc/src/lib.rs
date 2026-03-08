//! qverc - Quantum Version Control
//!
//! A DAG-based version control system optimized for AI agent workflows.

pub mod cli;
pub mod core;
pub mod gatekeeper;
pub mod storage;

pub use core::config::Config;
pub use core::graph::Graph;
pub use core::node::{Manifest, Node, NodeStatus, Zone};
pub use gatekeeper::Gatekeeper;
pub use storage::cas::ContentStore;
pub use storage::database::Database;

