//! `qverc edit` command
//!
//! Starts editing with an intent, forking from the current HEAD.

use crate::cli::init::{find_qvern_root, WorkspaceState};
use anyhow::Result;
use colored::Colorize;

/// Run the edit command
pub fn run(intent: &str) -> Result<()> {
    let _root = find_qvern_root()?;

    // Load current workspace state
    let mut state = WorkspaceState::load()?;

    let base_node = state.current_node.clone();

    // Update workspace state with new intent
    state.intent = if intent.is_empty() {
        None
    } else {
        Some(intent.to_string())
    };
    state.save()?;

    println!(
        "{} Starting edit session",
        "→".blue()
    );

    if let Some(ref node_id) = base_node {
        println!("  Base node: {}", node_id.cyan());
    } else {
        println!("  {} No existing nodes (fresh repository)", "note:".blue());
    }

    if let Some(ref intent) = state.intent {
        println!("  Intent: {}", intent.green());
    }

    println!();
    println!("Workspace is ready for editing.");
    println!("When done, run {} to commit your changes.", "qverc sync".cyan());

    Ok(())
}
