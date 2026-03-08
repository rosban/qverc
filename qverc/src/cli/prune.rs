//! `qverc prune` command
//!
//! Garbage collection for exploration nodes.

use crate::cli::init::{db_path, qvern_dir};
use crate::core::graph::Graph;
use crate::storage::cas::ContentStore;
use crate::storage::database::Database;
use anyhow::Result;
use chrono::{Duration, Utc};
use colored::Colorize;
use std::collections::HashSet;

/// Run the prune command
pub fn run(
    older_than: Option<&str>,
    failed_only: bool,
    orphaned: bool,
    execute: bool,
) -> Result<()> {
    let db = Database::open(db_path()?)?;
    let mut graph = Graph::new(db);

    println!("{}", "qverc prune".bold());
    println!();

    // Collect candidates for pruning
    let mut candidates: HashSet<String> = HashSet::new();

    // Add orphaned nodes
    if orphaned || (!failed_only && older_than.is_none()) {
        let orphaned_nodes = graph.find_orphaned_nodes()?;
        for node_id in orphaned_nodes {
            candidates.insert(node_id);
        }
    }

    // Add failed nodes
    if failed_only {
        let failed_nodes = graph.find_failed_nodes()?;
        for node_id in failed_nodes {
            candidates.insert(node_id);
        }
    }

    // Filter by age
    if let Some(duration_str) = older_than {
        let duration = parse_duration(duration_str)?;
        let cutoff = Utc::now() - duration;
        let old_nodes = graph.database().find_nodes_older_than(cutoff.timestamp())?;

        if candidates.is_empty() {
            // If no other filter, start with old nodes
            candidates = old_nodes.into_iter().collect();
        } else {
            // Intersect with existing candidates
            let old_set: HashSet<_> = old_nodes.into_iter().collect();
            candidates = candidates.intersection(&old_set).cloned().collect();
        }
    }

    // If no filters specified, find all prunable nodes (orphaned in exploration zone)
    if candidates.is_empty() && !orphaned && !failed_only && older_than.is_none() {
        let orphaned_nodes = graph.find_orphaned_nodes()?;
        for node_id in orphaned_nodes {
            candidates.insert(node_id);
        }
    }

    if candidates.is_empty() {
        println!("  {} No nodes to prune", "✓".green());
        return Ok(());
    }

    // Sort for consistent output
    let mut to_prune: Vec<_> = candidates.into_iter().collect();
    to_prune.sort();

    println!("  Found {} node(s) to prune:", to_prune.len());
    for node_id in &to_prune {
        println!("    {}", node_id.dimmed());
    }

    if !execute {
        println!();
        println!(
            "  {} This is a dry run. Use {} to actually delete.",
            "note:".blue(),
            "--execute".cyan()
        );
        return Ok(());
    }

    // Execute pruning
    println!();
    println!("  {} Pruning nodes...", "→".blue());

    let mut deleted = 0;
    let mut errors = 0;

    for node_id in &to_prune {
        match graph.delete_node(node_id) {
            Ok(()) => {
                deleted += 1;
                println!("    {} {}", "deleted".red(), node_id);
            }
            Err(e) => {
                errors += 1;
                println!("    {} {} ({})", "error".red(), node_id, e);
            }
        }
    }

    // Garbage collect orphaned blobs
    println!();
    println!("  {} Collecting orphaned blobs...", "→".blue());

    let qvern = qvern_dir()?;
    let cas = ContentStore::new(&qvern);
    let gc_result = garbage_collect_blobs(&graph, &cas)?;

    println!(
        "  {} Deleted {} node(s), {} blob(s), {} error(s)",
        "done:".green().bold(),
        deleted,
        gc_result.blobs_deleted,
        errors
    );

    if gc_result.bytes_freed > 0 {
        println!(
            "  {} freed",
            format_bytes(gc_result.bytes_freed)
        );
    }

    Ok(())
}

struct GcResult {
    blobs_deleted: usize,
    bytes_freed: u64,
}

fn garbage_collect_blobs(graph: &Graph, cas: &ContentStore) -> Result<GcResult> {
    // Get all blobs in use
    let used_blobs: HashSet<_> = graph
        .database()
        .get_all_blob_hashes()?
        .into_iter()
        .collect();

    // Get all stored blobs
    let stored_blobs = cas.list_objects()?;

    let mut deleted = 0;
    let mut bytes_freed = 0u64;

    for blob_hash in stored_blobs {
        if !used_blobs.contains(&blob_hash) {
            // Get size before deleting
            if let Ok(data) = cas.retrieve(&blob_hash) {
                bytes_freed += data.len() as u64;
            }
            cas.delete(&blob_hash)?;
            deleted += 1;
        }
    }

    Ok(GcResult {
        blobs_deleted: deleted,
        bytes_freed,
    })
}

fn parse_duration(s: &str) -> Result<Duration> {
    let s = s.trim().to_lowercase();

    if s.ends_with('d') {
        let days: i64 = s[..s.len() - 1].parse()?;
        return Ok(Duration::days(days));
    }
    if s.ends_with('h') {
        let hours: i64 = s[..s.len() - 1].parse()?;
        return Ok(Duration::hours(hours));
    }
    if s.ends_with('m') {
        let minutes: i64 = s[..s.len() - 1].parse()?;
        return Ok(Duration::minutes(minutes));
    }

    // Default to days
    let days: i64 = s.parse()?;
    Ok(Duration::days(days))
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

