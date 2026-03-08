//! qverc CLI entry point

use clap::{Parser, Subcommand};
use colored::Colorize;
use std::process;

mod cli;
mod core;
mod gatekeeper;
mod storage;

#[derive(Parser)]
#[command(name = "qverc")]
#[command(author, version, about = "Quantum Version Control - DAG-based VCS for AI workflows")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new qverc repository
    Init,

    /// Start editing with an intent (forks from HEAD)
    Edit {
        /// The intent/purpose of this edit
        #[arg(default_value = "")]
        intent: String,
    },

    /// Checkout a specific node (restore workspace to that state)
    Checkout {
        /// Node ID to checkout (can be partial)
        node_id: String,

        /// Force checkout, discarding uncommitted changes
        #[arg(short, long)]
        force: bool,
    },

    /// Sync workspace to the graph (hash, verify, commit)
    Sync {
        /// Agent signature for this sync
        #[arg(short, long)]
        agent: Option<String>,

        /// Skip gatekeeper verification
        #[arg(long)]
        skip_verify: bool,
    },

    /// Show current workspace status
    Status {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Show the DAG history
    Log {
        /// Number of nodes to show
        #[arg(short, long, default_value = "10")]
        limit: usize,

        /// Show all zones (including exploration)
        #[arg(short, long)]
        all: bool,

        /// Output in JSON format (for tooling integration)
        #[arg(long)]
        json: bool,
    },

    /// Promote a node to the Spine (consolidation zone)
    Promote {
        /// Node ID to promote (defaults to HEAD)
        node_id: Option<String>,

        /// Skip Tier 3 verification
        #[arg(long)]
        skip_verify: bool,

        /// Force promotion even if node is in draft status
        #[arg(short, long)]
        force: bool,
    },

    /// Prune exploration nodes (garbage collection)
    Prune {
        /// Only prune nodes older than this duration (e.g., "7d", "24h")
        #[arg(long)]
        older_than: Option<String>,

        /// Only prune failed/draft nodes
        #[arg(long)]
        failed_only: bool,

        /// Only prune orphaned nodes (no children, not HEAD)
        #[arg(long)]
        orphaned: bool,

        /// Actually delete (without this, only shows what would be deleted)
        #[arg(long)]
        execute: bool,
    },

    /// Query the repository (basic file path search)
    Query {
        /// Search pattern
        pattern: String,
    },

    /// Merge multiple nodes into a new state
    Merge {
        #[command(subcommand)]
        action: MergeAction,
    },

    /// Squash a linear sequence of nodes into a single node
    Squash {
        /// Start node ID (first node in the range to squash)
        start_node: String,

        /// End node ID (last node in the range to squash)
        end_node: String,

        /// Include spine node (only allowed if end_node is on spine)
        #[arg(long)]
        include_spine: bool,

        /// Custom intent for the squashed node (default: combined intents)
        #[arg(short, long)]
        intent: Option<String>,
    },

    /// Manage multiple working directories
    Worktree {
        #[command(subcommand)]
        action: WorktreeAction,
    },
}

#[derive(Subcommand)]
enum MergeAction {
    /// Prepare a merge from multiple nodes
    Run {
        /// Node IDs to merge (at least 2 required)
        #[arg(required = true)]
        nodes: Vec<String>,

        /// Intent for the merged state
        #[arg(short, long)]
        intent: Option<String>,
    },

    /// Show current merge status
    Status,

    /// Abort the current merge
    Abort,
}

#[derive(Subcommand)]
enum WorktreeAction {
    /// Add a new linked worktree
    Add {
        /// Path where the worktree will be created
        path: String,

        /// Node ID to check out (defaults to current HEAD)
        #[arg(short, long)]
        node: Option<String>,

        /// Name for the worktree (defaults to directory name)
        #[arg(long)]
        name: Option<String>,
    },

    /// List all worktrees
    List,

    /// Remove a linked worktree
    Remove {
        /// Path to the worktree to remove
        path: String,

        /// Force removal even if worktree has modifications
        #[arg(short, long)]
        force: bool,
    },

    /// Prune metadata for deleted worktrees
    Prune,
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Init => cli::init::run(),
        Commands::Edit { intent } => cli::edit::run(&intent),
        Commands::Checkout { node_id, force } => cli::checkout::run(&node_id, force),
        Commands::Sync { agent, skip_verify } => cli::sync::run(agent.as_deref(), skip_verify),
        Commands::Status { json } => cli::status::run(json),
        Commands::Log { limit, all, json } => cli::log::run(limit, all, json),
        Commands::Promote {
            node_id,
            skip_verify,
            force,
        } => cli::promote::run(node_id.as_deref(), skip_verify, force),
        Commands::Prune {
            older_than,
            failed_only,
            orphaned,
            execute,
        } => cli::prune::run(older_than.as_deref(), failed_only, orphaned, execute),
        Commands::Query { pattern } => cli::query::run(&pattern),
        Commands::Merge { action } => match action {
            MergeAction::Run { nodes, intent } => cli::merge::run(&nodes, intent.as_deref()),
            MergeAction::Status => cli::merge::status(),
            MergeAction::Abort => cli::merge::abort(),
        },
        Commands::Squash {
            start_node,
            end_node,
            include_spine,
            intent,
        } => cli::squash::run(&start_node, &end_node, include_spine, intent.as_deref()),
        Commands::Worktree { action } => match action {
            WorktreeAction::Add { path, node, name } => {
                cli::worktree::run_add(&path, node.as_deref(), name.as_deref())
            }
            WorktreeAction::List => cli::worktree::run_list(),
            WorktreeAction::Remove { path, force } => cli::worktree::run_remove(&path, force),
            WorktreeAction::Prune => cli::worktree::run_prune(),
        },
    };

    if let Err(e) = result {
        eprintln!("{} {}", "error:".red().bold(), e);
        process::exit(1);
    }
}

