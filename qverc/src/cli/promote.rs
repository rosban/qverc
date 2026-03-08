//! `qverc promote` command
//!
//! Promotes a node from the Exploration Zone to the Consolidation Zone (Spine).
//! Runs Tier 3 verification before promotion.

use crate::cli::init::{db_path, find_qvern_root, WorkspaceState};
use crate::core::config::Config;
use crate::core::node::{NodeStatus, Zone};
use crate::gatekeeper::{Gatekeeper, Tier};
use crate::storage::database::Database;
use anyhow::Result;
use colored::Colorize;

/// Run the promote command
pub fn run(node_id: Option<&str>, skip_verify: bool, force: bool) -> Result<()> {
    let root = find_qvern_root()?;

    // Open database
    let mut db = Database::open(db_path()?)?;

    // Resolve which node to promote
    let target_node_id = match node_id {
        Some(id) => resolve_node_id(&db, id)?,
        None => {
            let state = WorkspaceState::load()?;
            state.current_node
                .ok_or_else(|| anyhow::anyhow!("No current node. Run qverc sync first or specify a node ID."))?
        }
    };

    println!(
        "{} Promoting node {}...",
        "→".blue(),
        target_node_id.cyan()
    );

    // Get node details
    let manifest = db
        .get_manifest(&target_node_id)?
        .ok_or_else(|| anyhow::anyhow!("Node {} not found", target_node_id))?;

    // Check if already on spine
    if manifest.zone == Zone::Consolidation {
        println!(
            "{} Node {} is already on the Spine",
            "note:".blue(),
            target_node_id.cyan()
        );
        return Ok(());
    }

    // Check current status
    println!("  Current status: {}", format_status(manifest.status));
    println!("  Current zone: {}", manifest.zone);

    // Warn if node is not already verified
    if manifest.status == NodeStatus::Draft && !force {
        anyhow::bail!(
            "Node {} is in draft status. Run verification first or use --force",
            target_node_id
        );
    }

    // Load config for gatekeeper
    let config = Config::load_from_repo(&root).unwrap_or_default();
    let gatekeeper = Gatekeeper::new(config.clone());

    // Run Tier 3 verification unless skipped
    if !skip_verify {
        if gatekeeper.has_commands(Tier::Tier3) {
            println!();
            println!("{} Running Tier 3 verification...", "→".blue());

            let result = gatekeeper.verify(Tier::Tier3, &root)?;

            for output in &result.outputs {
                let status_icon = if output.exit_code == 0 {
                    "✓".green()
                } else {
                    "✗".red()
                };
                println!(
                    "  {} {} ({}ms)",
                    status_icon,
                    output.command,
                    output.duration_ms
                );
            }

            if !result.passed {
                anyhow::bail!("Tier 3 verification failed. Cannot promote to Spine.");
            }

            println!("  {} Tier 3 passed", "✓".green());
        } else {
            println!(
                "{} No Tier 3 commands configured, skipping verification",
                "→".blue()
            );
        }
    } else {
        println!("{} Skipping verification", "→".blue());
    }

    // Promote the node
    println!();
    println!("{} Updating node...", "→".blue());

    // Update status to Spine
    db.update_node_status(&target_node_id, NodeStatus::Spine)?;

    // Update zone to Consolidation
    db.update_node_zone(&target_node_id, Zone::Consolidation)?;

    // Update SPINE ref to point to this node
    db.set_ref("SPINE", &target_node_id)?;

    println!();
    println!(
        "{} Node {} promoted to Spine!",
        "success:".green().bold(),
        target_node_id.cyan()
    );

    if let Some(ref intent) = manifest.intent_prompt {
        println!("  Intent: {}", intent);
    }

    println!("  Status: {}", format_status(NodeStatus::Spine));
    println!("  Zone: {}", "consolidation".cyan());

    Ok(())
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

fn format_status(status: NodeStatus) -> colored::ColoredString {
    match status {
        NodeStatus::Draft => "draft".red(),
        NodeStatus::Valid => "valid".yellow(),
        NodeStatus::Verified => "verified".green(),
        NodeStatus::Spine => "spine".cyan().bold(),
    }
}
