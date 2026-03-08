//! `qverc checkout` command
//!
//! Restores the workspace to a specific node state.

use crate::cli::init::{db_path, find_qvern_root, qvern_dir, WorkspaceState};
use crate::core::config::Config;
use crate::core::graph::Graph;
use crate::storage::cas::ContentStore;
use crate::storage::database::Database;
use anyhow::{Context, Result};
use colored::Colorize;
use ignore::overrides::OverrideBuilder;
use ignore::WalkBuilder;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// Run the checkout command
pub fn run(node_id: &str, force: bool) -> Result<()> {
    let root = find_qvern_root()?;
    let qvern = qvern_dir()?;

    // Load config for ignore patterns
    let config = Config::load_from_repo(&root).unwrap_or_default();

    // Open database and graph
    let db = Database::open(db_path()?)?;
    let graph = Graph::new(db);

    // Resolve node ID (support short IDs)
    let full_node_id = resolve_node_id(&graph, node_id)?;

    println!(
        "{} Checking out {}...",
        "→".blue(),
        full_node_id.cyan()
    );

    // Get the node
    let node = graph
        .get_node(&full_node_id)
        .context(format!("Node {} not found", full_node_id))?;

    // Check for uncommitted changes unless --force
    if !force {
        let state = WorkspaceState::load()?;
        let current_node = state.current_node.or(graph.get_head()?);

        if let Some(ref current_id) = current_node {
            if has_uncommitted_changes(&root, &graph, &config, current_id)? {
                anyhow::bail!(
                    "You have uncommitted changes. Use --force to discard them, or run 'qverc sync' first."
                );
            }
        }
    }

    // Get the CAS
    let cas = ContentStore::new(&qvern);

    // Collect current workspace files (to detect deletions)
    let mut current_files: HashSet<String> = HashSet::new();
    collect_workspace_files(&root, &config, &mut current_files)?;

    // Materialize the node's files
    let mut restored = 0;
    for file in &node.files {
        let file_path = root.join(&file.path);

        // Create parent directories
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Retrieve content from CAS
        let content = cas
            .retrieve(&file.blob_hash)
            .context(format!("Failed to retrieve blob {} for {}", &file.blob_hash[..8], file.path))?;

        // Write file
        fs::write(&file_path, content)?;

        // Set permissions on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(file.mode);
            let _ = fs::set_permissions(&file_path, perms);
        }

        current_files.remove(&file.path);
        restored += 1;
    }

    // Remove files that don't exist in the target node
    let mut removed = 0;
    for orphan_path in &current_files {
        let file_path = root.join(orphan_path);
        if file_path.exists() {
            fs::remove_file(&file_path)?;
            removed += 1;
            println!("  {} {}", "removed".red(), orphan_path);
        }
    }

    // Clean up empty directories
    cleanup_empty_dirs(&root)?;

    // Update workspace state
    let mut state = WorkspaceState::load()?;
    state.current_node = Some(full_node_id.clone());
    state.intent = None;
    // Clear any merge state - checking out means we're leaving the merge
    state.clear_merge();
    state.save()?;

    // Update HEAD
    let mut db = Database::open(db_path()?)?;
    db.set_ref("HEAD", &full_node_id)?;

    println!();
    println!(
        "{} Checked out {} ({} files restored{})",
        "success:".green().bold(),
        full_node_id.cyan(),
        restored,
        if removed > 0 {
            format!(", {} removed", removed)
        } else {
            String::new()
        }
    );

    if let Some(ref intent) = node.manifest.intent_prompt {
        println!("  Intent: {}", intent);
    }

    Ok(())
}

/// Resolve a potentially short node ID to full ID
fn resolve_node_id(graph: &Graph, partial_id: &str) -> Result<String> {
    // If it looks like a full ID, verify it exists
    if partial_id.starts_with("qv-") && partial_id.len() == 9 {
        if graph.node_exists(partial_id)? {
            return Ok(partial_id.to_string());
        }
    }

    // Try to find a matching node
    let db = graph.database();
    let nodes = db.get_recent_nodes(1000, true)?; // Get all nodes

    let matches: Vec<_> = nodes
        .iter()
        .filter(|n| n.node_id.contains(partial_id))
        .collect();

    match matches.len() {
        0 => anyhow::bail!("No node found matching '{}'", partial_id),
        1 => Ok(matches[0].node_id.clone()),
        _ => {
            let ids: Vec<_> = matches.iter().map(|n| n.node_id.as_str()).collect();
            anyhow::bail!(
                "Ambiguous node ID '{}'. Matches: {}",
                partial_id,
                ids.join(", ")
            );
        }
    }
}

/// Check if there are uncommitted changes
fn has_uncommitted_changes(
    root: &Path,
    graph: &Graph,
    config: &Config,
    current_node_id: &str,
) -> Result<bool> {
    let node_files: HashSet<(String, String)> = if let Ok(files) = graph.get_files(current_node_id)
    {
        files.into_iter().map(|f| (f.path, f.blob_hash)).collect()
    } else {
        return Ok(false);
    };

    // Scan workspace
    let mut workspace_files: HashSet<String> = HashSet::new();
    collect_workspace_files(root, config, &mut workspace_files)?;

    // Check for modified or added files
    for path in &workspace_files {
        let file_path = root.join(path);
        let hash = ContentStore::hash_file(&file_path)?;

        let node_hash = node_files.iter().find(|(p, _)| p == path).map(|(_, h)| h);

        match node_hash {
            Some(h) if h == &hash => continue,
            _ => return Ok(true), // Modified or added
        }
    }

    // Check for deleted files
    for (path, _) in &node_files {
        if !workspace_files.contains(path) {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Collect all files in the workspace (respecting ignore patterns)
fn collect_workspace_files(root: &Path, config: &Config, files: &mut HashSet<String>) -> Result<()> {
    // Build override patterns for ignoring files
    let mut override_builder = OverrideBuilder::new(root);
    for pattern in &config.workspace.ignore {
        let neg_pattern = format!("!{}", pattern);
        override_builder.add(&neg_pattern)?;
    }
    override_builder.add("!.qverc/**")?;
    let overrides = override_builder.build()?;

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

        files.insert(rel_path);
    }

    Ok(())
}

/// Clean up empty directories after file removal
fn cleanup_empty_dirs(root: &Path) -> Result<()> {
    use walkdir::WalkDir;

    // Collect directories (deepest first)
    let mut dirs: Vec<_> = WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter(|e| !e.path().starts_with(root.join(".qverc")))
        .filter(|e| !e.path().starts_with(root.join(".git")))
        .map(|e| e.path().to_path_buf())
        .collect();

    // Sort by depth (deepest first)
    dirs.sort_by(|a, b| b.components().count().cmp(&a.components().count()));

    for dir in dirs {
        if dir == root {
            continue;
        }
        // Try to remove if empty
        let _ = fs::remove_dir(&dir);
    }

    Ok(())
}

