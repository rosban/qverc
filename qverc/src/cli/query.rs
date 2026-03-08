//! `qverc query` command
//!
//! Basic file path search (with placeholder for vector plugin).

use crate::cli::init::db_path;
use crate::storage::database::Database;
use anyhow::Result;
use colored::Colorize;
use std::collections::HashMap;

/// Run the query command
pub fn run(pattern: &str) -> Result<()> {
    let db = Database::open(db_path()?)?;

    println!("{} Searching for: {}", "→".blue(), pattern.cyan());
    println!();

    // Search files by path pattern
    let results = db.search_files(pattern)?;

    if results.is_empty() {
        println!("  {} No matches found", "note:".blue());
        return Ok(());
    }

    // Group by node
    let mut by_node: HashMap<String, Vec<String>> = HashMap::new();
    for (node_id, file) in &results {
        by_node
            .entry(node_id.clone())
            .or_default()
            .push(file.path.clone());
    }

    // Print results
    let mut nodes: Vec<_> = by_node.keys().collect();
    nodes.sort();

    for node_id in nodes {
        let files = &by_node[node_id];
        println!("  {} {}", "node".dimmed(), node_id.cyan());
        for path in files {
            println!("    {}", path);
        }
        println!();
    }

    println!(
        "  Found {} file(s) in {} node(s)",
        results.len(),
        by_node.len()
    );

    // Note about vector search
    println!();
    println!(
        "  {} For semantic search, configure a vector plugin in qverc.toml",
        "tip:".blue()
    );

    Ok(())
}

/// Plugin trait for vector search (for future extension)
#[allow(dead_code)]
pub trait VectorSearchPlugin: Send + Sync {
    /// Index a file's contents
    fn index(&mut self, node_id: &str, path: &str, content: &[u8]) -> Result<()>;

    /// Search for similar content
    fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>>;

    /// Remove indexed content for a node
    fn remove_node(&mut self, node_id: &str) -> Result<()>;
}

/// Search result from vector plugin
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub node_id: String,
    pub path: String,
    pub snippet: String,
    pub score: f32,
}

