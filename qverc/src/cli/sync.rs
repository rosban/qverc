//! `qverc sync` command
//!
//! Snapshots the workspace, runs the Gatekeeper, and pushes to the graph.

use crate::cli::init::{db_path, find_qvern_root, qvern_dir, WorkspaceState};
use crate::cli::merge::MergeManifest;
use crate::core::config::Config;
use crate::core::graph::Graph;
use crate::core::node::{generate_node_id, FileEntry, Node, NodeStatus};
use crate::gatekeeper::{Gatekeeper, Tier};
use crate::storage::cas::{hash_tree, ContentStore};
use crate::storage::database::Database;
use anyhow::{bail, Context, Result};
use chrono::Utc;
use colored::Colorize;
use ignore::overrides::OverrideBuilder;
use ignore::WalkBuilder;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// Run the sync command
pub fn run(agent: Option<&str>, skip_verify: bool) -> Result<()> {
    let root = find_qvern_root()?;
    let qvern = qvern_dir()?;

    // Load config
    let config = Config::load_from_repo(&root).unwrap_or_default();

    // Load workspace state
    let workspace_state = WorkspaceState::load()?;

    // Check for pending merge with unresolved conflicts
    let is_merge = workspace_state.is_merge_pending();
    if is_merge {
        if let Some(manifest) = MergeManifest::load()? {
            // Check which conflicts are actually unresolved
            // A conflict is resolved if the file exists in the workspace
            let mut truly_unresolved: Vec<&String> = Vec::new();
            for path in manifest.unresolved_conflicts() {
                let file_path = root.join(path);
                if !file_path.exists() {
                    truly_unresolved.push(path);
                }
            }
            
            if !truly_unresolved.is_empty() {
                println!("{} Merge has unresolved conflicts:", "error:".red().bold());
                for path in &truly_unresolved {
                    println!("  {} {}", "•".yellow(), path);
                }
                println!();
                println!("Resolution options:");
                println!("  1. Create merged files in the workspace for each conflict");
                println!("  2. Run {} to abort the merge", "qverc merge abort".cyan());
                println!();
                println!("Conflict versions are in: {}", ".qverc/merge/files/".cyan());
                bail!("Cannot sync with unresolved merge conflicts");
            }
        }
    }

    println!("{} Scanning workspace...", "→".blue());

    // Scan workspace and store files
    let cas = ContentStore::new(&qvern);
    let (files, tree_entries) = scan_workspace(&root, &cas, &config)?;

    if files.is_empty() {
        println!("{} No files to sync", "warning:".yellow().bold());
        return Ok(());
    }

    // Compute tree hash
    let tree_hash = hash_tree(&tree_entries);

    println!(
        "  {} {} files, tree hash: {}",
        "found".green(),
        files.len(),
        &tree_hash[..12]
    );

    // Determine parent nodes
    let parents = if let Some(ref merge_parents) = workspace_state.merge_parents {
        // This is a merge - use the merge parents
        println!("  {} merge with {} parents", "→".blue(), merge_parents.len());
        merge_parents.clone()
    } else if let Some(ref current) = workspace_state.current_node {
        vec![current.clone()]
    } else {
        // Check for HEAD
        let db = Database::open(db_path()?)?;
        let graph = Graph::new(db);
        if let Some(head) = graph.get_head()? {
            vec![head]
        } else {
            vec![]
        }
    };

    // Generate node ID
    let node_id = generate_node_id(&parents, &tree_hash, Utc::now());

    println!(
        "{} Creating node {}...",
        "→".blue(),
        node_id.cyan()
    );

    // Create the node
    let mut node = Node::new(node_id.clone(), parents, tree_hash, files);

    // Set intent and agent
    if let Some(intent) = &workspace_state.intent {
        node = node.with_intent(intent.clone());
    }
    if let Some(agent) = agent {
        node = node.with_agent(agent);
    }

    // Run verification unless skipped
    let final_status = if skip_verify {
        println!("{} Skipping verification", "→".blue());
        NodeStatus::Draft
    } else {
        run_verification(&root, &config)?
    };

    node = node.with_status(final_status);

    // Add to graph
    let db = Database::open(db_path()?)?;
    let mut graph = Graph::new(db);
    
    graph.add_node(&node).context("Failed to add node to graph")?;

    // Update workspace state
    let mut new_state = workspace_state;
    new_state.current_node = Some(node_id.clone());
    new_state.intent = None; // Clear intent after sync
    
    // If this was a merge, clear merge state and cleanup
    if is_merge {
        new_state.clear_merge();
        MergeManifest::cleanup()?;
    }
    
    new_state.save()?;

    println!();
    println!(
        "{} Node {} created",
        "success:".green().bold(),
        node_id.cyan()
    );
    println!("  Status: {}", format_status(final_status));
    if let Some(intent) = &node.manifest.intent_prompt {
        println!("  Intent: {}", intent);
    }

    Ok(())
}

/// Scan the workspace and store files in CAS
fn scan_workspace(
    root: &Path,
    cas: &ContentStore,
    config: &Config,
) -> Result<(Vec<FileEntry>, Vec<(String, String)>)> {
    let mut files = Vec::new();
    let mut tree_entries = Vec::new();

    // Build override patterns for ignoring files
    let mut override_builder = OverrideBuilder::new(root);
    
    // Add custom ignore patterns (negated to exclude them)
    for pattern in &config.workspace.ignore {
        // Convert to negated glob pattern
        let neg_pattern = format!("!{}", pattern);
        override_builder.add(&neg_pattern)?;
    }
    
    // Always ignore .qverc directory
    override_builder.add("!.qverc/**")?;
    
    let overrides = override_builder.build()?;

    // Build the walker with ignore patterns
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(true)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .add_custom_ignore_filename(".qvignore")
        .overrides(overrides);

    // Path for additional manual check
    let qverc_path = root.join(".qverc");

    for entry in builder.build() {
        let entry = entry?;
        let path = entry.path();

        // Skip directories
        if path.is_dir() {
            continue;
        }

        // Skip .qverc directory (belt and suspenders)
        if path.starts_with(&qverc_path) {
            continue;
        }

        // Get relative path
        let rel_path = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        // Skip qverc.toml (config should not be versioned in qverc)
        if rel_path == "qverc.toml" {
            continue;
        }

        // Get file mode
        let mode = path
            .metadata()
            .map(|m| m.permissions().mode())
            .unwrap_or(0o644);

        // Store file and get hash
        let hash = cas.store_file(path)?;

        tree_entries.push((rel_path.clone(), hash.clone()));
        files.push(FileEntry {
            path: rel_path,
            blob_hash: hash,
            mode,
        });
    }

    // Sort for determinism
    files.sort_by(|a, b| a.path.cmp(&b.path));
    tree_entries.sort_by(|a, b| a.0.cmp(&b.0));

    Ok((files, tree_entries))
}

/// Run the gatekeeper verification
fn run_verification(root: &Path, config: &Config) -> Result<NodeStatus> {
    let gatekeeper = Gatekeeper::new(config.clone());

    // Check if any commands are configured
    if !gatekeeper.has_commands(Tier::Tier1)
        && !gatekeeper.has_commands(Tier::Tier2)
        && !gatekeeper.has_commands(Tier::Tier3)
    {
        println!("{} No verification commands configured", "→".blue());
        return Ok(NodeStatus::Draft);
    }

    println!("{} Running verification...", "→".blue());

    // Run tier 1
    if gatekeeper.has_commands(Tier::Tier1) {
        println!("  {} Tier 1 (syntax/linter)...", "→".blue());
        let result = gatekeeper.verify(Tier::Tier1, root)?;

        for output in &result.outputs {
            let status_icon = if output.exit_code == 0 {
                "✓".green()
            } else {
                "✗".red()
            };
            println!(
                "    {} {} ({}ms)",
                status_icon,
                output.command,
                output.duration_ms
            );
        }

        if !result.passed {
            println!("  {} Tier 1 failed", "✗".red());
            return Ok(NodeStatus::Draft);
        }
        println!("  {} Tier 1 passed", "✓".green());
    }

    // Run tier 2
    if gatekeeper.has_commands(Tier::Tier2) {
        println!("  {} Tier 2 (unit tests)...", "→".blue());
        let result = gatekeeper.verify(Tier::Tier2, root)?;

        for output in &result.outputs {
            let status_icon = if output.exit_code == 0 {
                "✓".green()
            } else {
                "✗".red()
            };
            println!(
                "    {} {} ({}ms)",
                status_icon,
                output.command,
                output.duration_ms
            );
        }

        if !result.passed {
            println!("  {} Tier 2 failed", "✗".red());
            return Ok(NodeStatus::Valid);
        }
        println!("  {} Tier 2 passed", "✓".green());
    }

    // We don't run tier 3 automatically (requires explicit promotion)
    // Return verified status if tier 2 passed
    if gatekeeper.has_commands(Tier::Tier2) {
        Ok(NodeStatus::Verified)
    } else if gatekeeper.has_commands(Tier::Tier1) {
        Ok(NodeStatus::Valid)
    } else {
        Ok(NodeStatus::Draft)
    }
}

fn format_status(status: NodeStatus) -> colored::ColoredString {
    match status {
        NodeStatus::Draft => "draft".red(),
        NodeStatus::Valid => "valid".yellow(),
        NodeStatus::Verified => "verified".green(),
        NodeStatus::Spine => "spine".cyan().bold(),
    }
}
