//! `qverc status` command
//!
//! Shows current workspace status.

use crate::cli::init::{db_path, find_qvern_root, qvern_dir, WorkspaceState};
use crate::core::config::Config;
use crate::core::graph::Graph;
use crate::core::node::NodeStatus;
use crate::storage::cas::ContentStore;
use crate::storage::database::Database;
use anyhow::Result;
use colored::Colorize;
use ignore::overrides::OverrideBuilder;
use ignore::WalkBuilder;
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;

/// JSON output structure for status command
#[derive(Serialize)]
struct StatusOutput {
    head: Option<String>,
    status: String,
    zone: String,
    changes: Vec<ChangeOutput>,
    #[serde(rename = "graphNodes")]
    graph_nodes: usize,
    #[serde(rename = "mergeInProgress")]
    merge_in_progress: bool,
    #[serde(rename = "mergeParents", skip_serializing_if = "Option::is_none")]
    merge_parents: Option<Vec<String>>,
}

#[derive(Serialize)]
struct ChangeOutput {
    path: String,
    #[serde(rename = "type")]
    change_type: String,
}

/// Run the status command
pub fn run(json: bool) -> Result<()> {
    let root = find_qvern_root()?;
    let qvern = qvern_dir()?;

    // Load state
    let state = WorkspaceState::load()?;
    let config = Config::load_from_repo(&root).unwrap_or_default();

    // Open database
    let db = Database::open(db_path()?)?;
    let graph = Graph::new(db);

    let current_node = state.current_node.as_ref();

    // Get node status and zone
    let (node_status, zone) = if let Some(node_id) = current_node {
        if let Ok(manifest) = graph.get_manifest(node_id) {
            (format!("{}", manifest.status).to_lowercase(), format!("{}", manifest.zone).to_lowercase())
        } else {
            ("unknown".to_string(), "unknown".to_string())
        }
    } else {
        ("unknown".to_string(), "unknown".to_string())
    };

    // Scan workspace for changes
    let cas = ContentStore::new(&qvern);
    let changes = detect_changes(&root, &graph, &cas, &config, current_node)?;
    let graph_nodes = graph.count_nodes()?;

    if json {
        // JSON output
        let output = StatusOutput {
            head: state.current_node.clone(),
            status: if state.is_merge_pending() { "merge".to_string() } else { node_status },
            zone,
            changes: changes
                .iter()
                .map(|c| match c {
                    Change::Added(p) => ChangeOutput {
                        path: p.clone(),
                        change_type: "added".to_string(),
                    },
                    Change::Modified(p) => ChangeOutput {
                        path: p.clone(),
                        change_type: "modified".to_string(),
                    },
                    Change::Deleted(p) => ChangeOutput {
                        path: p.clone(),
                        change_type: "deleted".to_string(),
                    },
                })
                .collect(),
            graph_nodes,
            merge_in_progress: state.is_merge_pending(),
            merge_parents: state.merge_parents.clone(),
        };
        println!("{}", serde_json::to_string(&output)?);
        return Ok(());
    }

    // Human-readable output
    println!("{}", "qverc status".bold());
    println!();

    // Check for merge in progress
    if state.is_merge_pending() {
        if let Some(ref merge_parents) = state.merge_parents {
            println!("  {} MERGE IN PROGRESS", "⚠".yellow().bold());
            println!("  Merging: {}", merge_parents.iter()
                .map(|s| s.cyan().to_string())
                .collect::<Vec<_>>()
                .join(" + "));
            if let Some(ref pre) = state.pre_merge_node {
                println!("  Pre-merge: {}", pre.dimmed());
            }
        }
    } else {
        match &state.current_node {
            Some(current) => {
                println!("  HEAD: {}", current.cyan());
            }
            None => {
                println!("  {} No commits yet", "note:".blue());
            }
        }

        // Show current node status
        if let Some(ref node_id) = current_node {
            if let Ok(manifest) = graph.get_manifest(node_id) {
                println!("  Status: {}", format_status(manifest.status));
                println!("  Zone: {}", manifest.zone);
            }
        }
    }

    // Show intent
    if let Some(ref intent) = state.intent {
        println!("  Intent: {}", intent.green());
    }

    println!();

    if changes.is_empty() {
        println!("  {} No changes detected", "✓".green());
    } else {
        println!("  Changes:");
        for change in &changes {
            match change {
                Change::Added(path) => println!("    {} {}", "+".green(), path),
                Change::Modified(path) => println!("    {} {}", "~".yellow(), path),
                Change::Deleted(path) => println!("    {} {}", "-".red(), path),
            }
        }
        println!();
        println!(
            "  Run {} to commit {} change(s)",
            "qverc sync".cyan(),
            changes.len()
        );
    }

    // Show graph stats
    println!();
    println!("  Graph: {} node(s)", graph_nodes);

    Ok(())
}

#[derive(Debug)]
enum Change {
    Added(String),
    Modified(String),
    Deleted(String),
}

fn detect_changes(
    root: &Path,
    graph: &Graph,
    _cas: &ContentStore,
    config: &Config,
    current_node: Option<&String>,
) -> Result<Vec<Change>> {
    let mut changes = Vec::new();

    // Get current files from node (if any)
    let node_files: HashSet<(String, String)> = if let Some(node_id) = current_node {
        if let Ok(files) = graph.get_files(node_id) {
            files.into_iter().map(|f| (f.path, f.blob_hash)).collect()
        } else {
            HashSet::new()
        }
    } else {
        HashSet::new()
    };

    // Build override patterns for ignoring files
    let mut override_builder = OverrideBuilder::new(root);
    for pattern in &config.workspace.ignore {
        let neg_pattern = format!("!{}", pattern);
        override_builder.add(&neg_pattern)?;
    }
    override_builder.add("!.qverc/**")?;
    let overrides = override_builder.build()?;

    // Scan workspace
    let mut workspace_files: HashSet<String> = HashSet::new();

    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(true)
        .git_ignore(false)
        .add_custom_ignore_filename(".qvignore")
        .overrides(overrides);

    let qverc_path = root.join(".qverc");

    for entry in builder.build() {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() || path.starts_with(&qverc_path) {
            continue;
        }

        let rel_path = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        if rel_path == "qverc.toml" {
            continue;
        }

        workspace_files.insert(rel_path.clone());

        // Compute hash
        let hash = ContentStore::hash_file(path)?;

        // Check if file exists in node
        let node_hash = node_files.iter().find(|(p, _)| p == &rel_path).map(|(_, h)| h);

        match node_hash {
            Some(h) if h == &hash => {
                // Unchanged
            }
            Some(_) => {
                changes.push(Change::Modified(rel_path));
            }
            None => {
                changes.push(Change::Added(rel_path));
            }
        }
    }

    // Check for deleted files
    for (path, _) in &node_files {
        if !workspace_files.contains(path) {
            changes.push(Change::Deleted(path.clone()));
        }
    }

    // Sort for consistent output
    changes.sort_by(|a, b| {
        let path_a = match a {
            Change::Added(p) | Change::Modified(p) | Change::Deleted(p) => p,
        };
        let path_b = match b {
            Change::Added(p) | Change::Modified(p) | Change::Deleted(p) => p,
        };
        path_a.cmp(path_b)
    });

    Ok(changes)
}

fn format_status(status: NodeStatus) -> colored::ColoredString {
    match status {
        NodeStatus::Draft => "draft".red(),
        NodeStatus::Valid => "valid".yellow(),
        NodeStatus::Verified => "verified".green(),
        NodeStatus::Spine => "spine".cyan().bold(),
    }
}
