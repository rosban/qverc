//! `qverc squash` command
//!
//! Squashes a linear sequence of nodes into a single node.
//! The original nodes are deleted and their blobs are pruned if unreferenced.

use crate::cli::init::{db_path, find_qvern_root, qvern_dir};
use crate::core::node::{generate_node_id, Node, Zone};
use crate::storage::cas::ContentStore;
use crate::storage::database::Database;
use anyhow::{bail, Result};
use chrono::Utc;
use colored::Colorize;
use std::collections::HashSet;

/// Run the squash command
pub fn run(start_node: &str, end_node: &str, include_spine: bool, intent: Option<&str>) -> Result<()> {
    let _root = find_qvern_root()?;
    let mut db = Database::open(db_path()?)?;

    // Resolve node IDs
    let start_id = resolve_node_id(&db, start_node)?;
    let end_id = resolve_node_id(&db, end_node)?;

    println!(
        "{} Squashing nodes {} → {}",
        "→".blue(),
        start_id.cyan(),
        end_id.cyan()
    );

    // Get start and end manifests
    let start_manifest = db
        .get_manifest(&start_id)?
        .ok_or_else(|| anyhow::anyhow!("Start node {} not found", start_id))?;
    let end_manifest = db
        .get_manifest(&end_id)?
        .ok_or_else(|| anyhow::anyhow!("End node {} not found", end_id))?;

    // Walk the path from start to end, collecting nodes
    let path = collect_linear_path(&db, &start_id, &end_id)?;
    println!("  Found {} nodes in path", path.len());

    // Verify path is linear (no forks or merges)
    for node_id in &path {
        let manifest = db.get_manifest(node_id)?.unwrap();
        
        // Check for multiple parents (merge node) - only the start node is allowed to have multiple parents
        if node_id != &start_id && manifest.parents.len() > 1 {
            bail!(
                "Cannot squash: node {} is a merge node with {} parents",
                node_id,
                manifest.parents.len()
            );
        }

        // Check for forks (multiple children, except for end node)
        if node_id != &end_id {
            let children = db.get_children(node_id)?;
            let children_in_path: Vec<_> = children.iter().filter(|c| path.contains(c)).collect();
            if children_in_path.len() != 1 {
                bail!(
                    "Cannot squash: path forks at node {} ({} children in path)",
                    node_id,
                    children_in_path.len()
                );
            }
        }
    }

    // Check spine constraints
    let mut has_spine = false;
    for node_id in &path {
        let manifest = db.get_manifest(node_id)?.unwrap();
        if manifest.zone == Zone::Consolidation {
            has_spine = true;
            if node_id != &end_id {
                bail!(
                    "Cannot squash: spine node {} is not the last node in path",
                    node_id
                );
            }
            if !include_spine {
                bail!(
                    "Cannot squash spine node {}. Use --include-spine to include it",
                    node_id
                );
            }
        }
    }

    // Determine the new node ID
    let new_node_id = if has_spine && include_spine {
        // Keep the spine node's ID
        end_id.clone()
    } else {
        // Generate a new ID
        generate_node_id(&start_manifest.parents, &end_manifest.tree_hash, Utc::now())
    };

    // Collect intents from all nodes
    let combined_intent = match intent {
        Some(i) => i.to_string(),
        None => {
            let intents: Vec<String> = path
                .iter()
                .filter_map(|id| {
                    db.get_manifest(id)
                        .ok()
                        .flatten()
                        .and_then(|m| m.intent_prompt)
                })
                .collect();
            if intents.is_empty() {
                "Squashed nodes".to_string()
            } else {
                intents.join(" | ")
            }
        }
    };

    println!("  Combined intent: {}", combined_intent);

    // Get the children of the end node (to re-parent later)
    let end_children = db.get_children(&end_id)?;

    // Get files from the end node (we keep its content)
    let end_files = db.get_files(&end_id)?;

    // Get the parents of the start node (new node will inherit these)
    let new_parents = start_manifest.parents.clone();

    println!();
    println!("{} Creating squashed node...", "→".blue());

    // If we're keeping the spine node's ID, we need to update it in place
    if has_spine && include_spine {
        // Update the spine node's parents FIRST to break edge references
        // This must happen before deleting intermediate nodes to avoid FK violations
        db.update_node_parents(&end_id, &new_parents)?;
        println!("  Updated {} parents → {:?}", end_id, new_parents);

        // Now delete all intermediate nodes (not end)
        // Delete in REVERSE order (from second-to-last toward start) to avoid FK violations
        // When we delete node X, its child (X+1) has either been deleted or is end_id
        for node_id in path.iter().rev() {
            if node_id != &end_id {
                db.delete_node(node_id)?;
                println!("  Deleted node {}", node_id);
            }
        }

        // Update the spine node's intent if a custom one was provided
        // For now, we update intent via a direct SQL update
        if intent.is_some() {
            db.update_node_intent(&end_id, &combined_intent)?;
            println!("  Updated intent for {}", end_id);
        }

        println!();
        println!(
            "{} Squashed into existing spine node {}",
            "success:".green().bold(),
            end_id.cyan()
        );
        println!("  Intent: {}", combined_intent);
    } else {
        // Create a new node with the squashed content
        let new_node = Node::new(
            new_node_id.clone(),
            new_parents,
            end_manifest.tree_hash.clone(),
            end_files,
        )
        .with_intent(&combined_intent)
        .with_status(end_manifest.status)
        .with_zone(end_manifest.zone);

        // Insert the new node
        db.insert_node(&new_node)?;
        println!("  Created node {}", new_node_id.cyan());

        // Re-parent children of end node to point to new node
        for child_id in &end_children {
            let child_parents = db.get_parents(child_id)?;
            let updated_parents: Vec<String> = child_parents
                .into_iter()
                .map(|p| if p == end_id { new_node_id.clone() } else { p })
                .collect();
            db.update_node_parents(child_id, &updated_parents)?;
            println!("  Re-parented {} → {}", child_id, new_node_id);
        }

        // Delete all original nodes in the path (reverse order to avoid FK violations)
        // We delete from end to start because edges reference parent_id
        for node_id in path.iter().rev() {
            db.delete_node(node_id)?;
            println!("  Deleted node {}", node_id);
        }

        // Update HEAD if it pointed to any deleted node
        if let Some(head) = db.get_ref("HEAD")? {
            if path.contains(&head) {
                db.set_ref("HEAD", &new_node_id)?;
                println!("  Updated HEAD → {}", new_node_id);
            }
        }

        println!();
        println!(
            "{} Squashed {} nodes into {}",
            "success:".green().bold(),
            path.len(),
            new_node_id.cyan()
        );
    }

    // Prune orphaned blobs
    println!();
    println!("{} Pruning orphaned blobs...", "→".blue());

    let cas = ContentStore::new(qvern_dir()?);
    let referenced_hashes: HashSet<String> = db.get_all_blob_hashes()?.into_iter().collect();
    let (deleted_blobs, bytes_freed) = cas.prune_orphaned(&referenced_hashes)?;

    if deleted_blobs > 0 {
        println!(
            "  Freed {} blobs ({} bytes)",
            deleted_blobs,
            format_bytes(bytes_freed)
        );
    } else {
        println!("  No orphaned blobs to prune");
    }

    Ok(())
}

/// Collect the linear path from start to end node
fn collect_linear_path(db: &Database, start_id: &str, end_id: &str) -> Result<Vec<String>> {
    let mut path = vec![start_id.to_string()];
    let mut current = start_id.to_string();

    // Walk forward from start to end
    while current != end_id {
        let children = db.get_children(&current)?;
        
        if children.is_empty() {
            bail!(
                "Cannot find path from {} to {}: {} has no children",
                start_id,
                end_id,
                current
            );
        }

        // Find the child that leads to end_id
        let mut found_next = false;
        for child in &children {
            if child == end_id || path_exists(db, child, end_id)? {
                path.push(child.clone());
                current = child.clone();
                found_next = true;
                break;
            }
        }

        if !found_next {
            bail!(
                "Cannot find path from {} to {}: no path from {}",
                start_id,
                end_id,
                current
            );
        }

        // Prevent infinite loops
        if path.len() > 1000 {
            bail!("Path too long (>1000 nodes). Possible cycle detected.");
        }
    }

    Ok(path)
}

/// Check if a path exists from source to target
fn path_exists(db: &Database, source: &str, target: &str) -> Result<bool> {
    let mut visited = HashSet::new();
    let mut stack = vec![source.to_string()];

    while let Some(current) = stack.pop() {
        if current == target {
            return Ok(true);
        }

        if visited.contains(&current) {
            continue;
        }
        visited.insert(current.clone());

        let children = db.get_children(&current)?;
        for child in children {
            if !visited.contains(&child) {
                stack.push(child);
            }
        }

        // Prevent excessive searching
        if visited.len() > 1000 {
            return Ok(false);
        }
    }

    Ok(false)
}

/// Resolve a potentially short node ID to full ID
fn resolve_node_id(db: &Database, partial_id: &str) -> Result<String> {
    // If it looks like a full ID, verify it exists
    if partial_id.starts_with("qv-") && partial_id.len() == 9 {
        if db.node_exists(partial_id)? {
            return Ok(partial_id.to_string());
        }
    }

    // Try to find a matching node
    let nodes = db.get_recent_nodes(1000, true)?;

    let matches: Vec<_> = nodes
        .iter()
        .filter(|n| n.node_id.contains(partial_id))
        .collect();

    match matches.len() {
        0 => bail!("No node found matching '{}'", partial_id),
        1 => Ok(matches[0].node_id.clone()),
        _ => {
            let ids: Vec<_> = matches.iter().map(|n| n.node_id.as_str()).collect();
            bail!(
                "Ambiguous node ID '{}'. Matches: {}",
                partial_id,
                ids.join(", ")
            );
        }
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

