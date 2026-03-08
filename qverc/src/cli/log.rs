//! `qverc log` command
//!
//! Displays the DAG history.

use crate::cli::init::{db_path, WorkspaceState};
use crate::core::graph::Graph;
use crate::core::node::{Manifest, NodeStatus, Zone};
use crate::storage::database::Database;
use anyhow::Result;
use chrono::{Local, TimeZone};
use colored::Colorize;
use serde::Serialize;

/// JSON output structure for log
#[derive(Serialize)]
struct LogOutput {
    head: Option<String>,
    nodes: Vec<NodeOutput>,
    total: usize,
}

#[derive(Serialize)]
struct NodeOutput {
    node_id: String,
    parents: Vec<String>,
    zone: String,
    status: String,
    intent: Option<String>,
    agent: Option<String>,
    tree_hash: String,
    created_at: String,
    is_head: bool,
}

/// Run the log command
pub fn run(limit: usize, show_all: bool, json: bool) -> Result<()> {
    let db = Database::open(db_path()?)?;
    let graph = Graph::new(db);

    let state = WorkspaceState::load()?;
    let head = state.current_node;
    let nodes = graph.get_recent_nodes(limit, show_all)?;

    if json {
        return run_json(&nodes, &head, graph.count_nodes()?);
    }

    if nodes.is_empty() {
        println!("{} No nodes in the graph", "note:".blue());
        return Ok(());
    }

    println!("{}", "qverc log".bold());
    println!();

    for manifest in nodes {
        print_node(&manifest, head.as_ref().map(|s| s == &manifest.node_id).unwrap_or(false))?;
        println!();
    }

    let total = graph.count_nodes()?;
    if total > limit {
        println!(
            "  {} Showing {} of {} nodes. Use {} to see more.",
            "...".dimmed(),
            limit,
            total,
            "--limit N".cyan()
        );
    }

    Ok(())
}

fn run_json(nodes: &[Manifest], head: &Option<String>, total: usize) -> Result<()> {
    let node_outputs: Vec<NodeOutput> = nodes
        .iter()
        .map(|m| {
            let is_head = head.as_ref().map(|h| h == &m.node_id).unwrap_or(false);
            NodeOutput {
                node_id: m.node_id.clone(),
                parents: m.parents.clone(),
                zone: match m.zone {
                    Zone::Exploration => "exploration".to_string(),
                    Zone::Consolidation => "consolidation".to_string(),
                },
                status: format!("{:?}", m.status).to_lowercase(),
                intent: m.intent_prompt.clone(),
                agent: m.agent_signature.clone(),
                tree_hash: m.tree_hash.clone(),
                created_at: m.created_at.to_rfc3339(),
                is_head,
            }
        })
        .collect();

    let output = LogOutput {
        head: head.clone(),
        nodes: node_outputs,
        total,
    };

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn print_node(manifest: &Manifest, is_head: bool) -> Result<()> {
    // Node ID with HEAD indicator
    let node_id = if is_head {
        format!("{} {}", manifest.node_id.cyan().bold(), "(HEAD)".yellow())
    } else {
        manifest.node_id.cyan().to_string()
    };
    println!("  {} {}", "node".dimmed(), node_id);

    // Zone and status
    let zone_str = match manifest.zone {
        Zone::Exploration => "exploration".dimmed(),
        Zone::Consolidation => "spine".cyan(),
    };
    let status_str = format_status(manifest.status);
    println!("  {} {} | {}", "    ".dimmed(), zone_str, status_str);

    // Parents
    if !manifest.parents.is_empty() {
        let parents: Vec<_> = manifest.parents.iter().map(|p| p.as_str()).collect();
        println!(
            "  {} parent: {}",
            "    ".dimmed(),
            parents.join(", ").dimmed()
        );
    }

    // Timestamp
    let local_time = Local.from_utc_datetime(&manifest.created_at.naive_utc());
    println!(
        "  {} {}",
        "    ".dimmed(),
        local_time.format("%Y-%m-%d %H:%M:%S").to_string().dimmed()
    );

    // Intent
    if let Some(ref intent) = manifest.intent_prompt {
        println!();
        println!("      {}", intent);
    }

    // Agent
    if let Some(ref agent) = manifest.agent_signature {
        println!("      {} {}", "agent:".dimmed(), agent.dimmed());
    }

    // Tree hash (abbreviated)
    println!(
        "      {} {}",
        "tree:".dimmed(),
        &manifest.tree_hash[..12].dimmed()
    );

    Ok(())
}

fn format_status(status: NodeStatus) -> colored::ColoredString {
    match status {
        NodeStatus::Draft => "draft".red(),
        NodeStatus::Valid => "valid".yellow(),
        NodeStatus::Verified => "verified".green(),
        NodeStatus::Spine => "spine".cyan().bold(),
    }
}
