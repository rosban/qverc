//! `qverc init` command
//!
//! Initializes a new qverc repository.

use crate::core::config::Config;
use crate::storage::cas::ContentStore;
use crate::storage::database::Database;
use anyhow::{Context, Result};
use colored::Colorize;
use std::fs;
use std::path::PathBuf;

const QVERC_DIR: &str = ".qverc";
const LEGACY_DIR: &str = ".qvern";

/// Migrate legacy .qvern directory to .qverc
fn migrate_legacy_dir(base_path: &PathBuf) -> Result<bool> {
    let legacy_path = base_path.join(LEGACY_DIR);
    let new_path = base_path.join(QVERC_DIR);

    if legacy_path.exists() && !new_path.exists() {
        println!(
            "{} Migrating {} → {}",
            "→".blue(),
            LEGACY_DIR,
            QVERC_DIR
        );
        fs::rename(&legacy_path, &new_path)
            .context("Failed to migrate .qvern to .qverc")?;
        
        // Also rename config file if it exists
        let legacy_config = base_path.join("qvern.toml");
        let new_config = base_path.join("qverc.toml");
        if legacy_config.exists() && !new_config.exists() {
            fs::rename(&legacy_config, &new_config)
                .context("Failed to migrate qvern.toml to qverc.toml")?;
            println!("  Renamed qvern.toml → qverc.toml");
        }
        
        return Ok(true);
    }
    Ok(false)
}

/// Run the init command
pub fn run() -> Result<()> {
    let cwd = std::env::current_dir()?;
    
    // Check for and migrate legacy .qvern directory
    migrate_legacy_dir(&cwd)?;
    
    let qverc_dir = cwd.join(QVERC_DIR);

    // Check if already initialized
    if qverc_dir.exists() {
        println!(
            "{} qverc repository already exists at {}",
            "warning:".yellow().bold(),
            qverc_dir.display()
        );
        return Ok(());
    }

    // Create .qverc directory
    fs::create_dir_all(&qverc_dir).context("Failed to create .qverc directory")?;

    // Initialize content store
    let cas = ContentStore::new(&qverc_dir);
    cas.init().context("Failed to initialize content store")?;

    // Initialize database
    let db_path = qverc_dir.join("db.sqlite");
    let db = Database::open(&db_path).context("Failed to open database")?;
    db.init_schema().context("Failed to initialize database schema")?;

    // Create default config
    let config_path = cwd.join("qverc.toml");
    if !config_path.exists() {
        let default_config = Config::default_toml();
        fs::write(&config_path, default_config).context("Failed to write qverc.toml")?;
        println!(
            "  {} {}",
            "created".green(),
            "qverc.toml"
        );
    }

    // Create ignore file
    let ignore_path = qverc_dir.join("ignore");
    let default_ignore = ".qverc/\n.qvern/\n.git/\ntarget/\nnode_modules/\n*.log\n.DS_Store\n";
    fs::write(&ignore_path, default_ignore).context("Failed to write ignore file")?;

    // Create workspace state file
    let state_path = qverc_dir.join("workspace.json");
    let initial_state = r#"{"current_node": null, "intent": null}"#;
    fs::write(&state_path, initial_state).context("Failed to write workspace state")?;

    println!(
        "{} Initialized qverc repository in {}",
        "success:".green().bold(),
        cwd.display()
    );
    println!();
    println!("Repository structure:");
    println!("  {}/.qverc/", cwd.display());
    println!("    ├── db.sqlite      (DAG database)");
    println!("    ├── objects/       (content store)");
    println!("    ├── ignore         (ignore patterns)");
    println!("    └── workspace.json (workspace state)");
    println!();
    println!("Next steps:");
    println!("  1. Configure verification commands in {}", "qverc.toml".cyan());
    println!("  2. Run {} to start editing", "qverc edit \"your intent\"".cyan());
    println!("  3. Run {} to commit your changes", "qverc sync".cyan());

    Ok(())
}

/// Find the qverc root directory (containing .qverc)
/// Works for both main worktrees (.qverc directory) and linked worktrees (.qverc file)
pub fn find_qvern_root() -> Result<PathBuf> {
    let info = get_worktree_info()?;
    Ok(info.worktree_root)
}

/// Get the .qverc directory path
pub fn qvern_dir() -> Result<PathBuf> {
    let info = get_worktree_info()?;
    Ok(info.main_repo)
}

/// Get the database path
pub fn db_path() -> Result<PathBuf> {
    Ok(qvern_dir()?.join("db.sqlite"))
}

/// Information about the current worktree
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    /// Path to the main .qverc directory (shared database)
    pub main_repo: PathBuf,
    /// Name of this worktree (empty string for main worktree)
    pub worktree_name: String,
    /// True if this is a linked worktree (not the main one)
    pub is_linked: bool,
    /// Path to the worktree root directory
    pub worktree_root: PathBuf,
}

/// Worktree link file content
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct WorktreeLink {
    pub main_repo: String,
    pub worktree_name: String,
}

/// Get information about the current worktree
pub fn get_worktree_info() -> Result<WorktreeInfo> {
    let mut current = std::env::current_dir()?;

    loop {
        // Try to migrate legacy directory first
        let _ = migrate_legacy_dir(&current);
        
        let qverc_path = current.join(QVERC_DIR);
        
        // Check if .qverc is a FILE (linked worktree)
        if qverc_path.exists() && qverc_path.is_file() {
            let content = fs::read_to_string(&qverc_path)
                .context("Failed to read .qverc link file")?;
            let link: WorktreeLink = serde_json::from_str(&content)
                .context("Failed to parse .qverc link file")?;
            
            let main_repo = PathBuf::from(&link.main_repo);
            if !main_repo.exists() {
                anyhow::bail!(
                    "Linked worktree points to non-existent main repo: {}",
                    main_repo.display()
                );
            }
            
            return Ok(WorktreeInfo {
                main_repo,
                worktree_name: link.worktree_name,
                is_linked: true,
                worktree_root: current,
            });
        }
        
        // Check if .qverc is a DIRECTORY (main worktree)
        if qverc_path.exists() && qverc_path.is_dir() {
            return Ok(WorktreeInfo {
                main_repo: qverc_path,
                worktree_name: String::new(),
                is_linked: false,
                worktree_root: current,
            });
        }

        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => anyhow::bail!("Not a qverc repository (or any parent up to root)"),
        }
    }
}

/// Get the path to the workspace state file for the current worktree
pub fn workspace_state_path() -> Result<PathBuf> {
    let info = get_worktree_info()?;
    
    if info.is_linked {
        // Linked worktrees store state in .qverc/worktrees/<name>/workspace.json
        Ok(info.main_repo.join("worktrees").join(&info.worktree_name).join("workspace.json"))
    } else {
        // Main worktree stores state directly in .qverc/workspace.json
        Ok(info.main_repo.join("workspace.json"))
    }
}

/// Check if we're in a linked worktree
pub fn is_linked_worktree() -> Result<bool> {
    Ok(get_worktree_info()?.is_linked)
}

/// Load the current workspace state
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceState {
    pub current_node: Option<String>,
    pub intent: Option<String>,
    /// Parent nodes for a pending merge operation
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub merge_parents: Option<Vec<String>>,
    /// The node we were on before starting a merge (for restore on abort)
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pre_merge_node: Option<String>,
}

impl WorkspaceState {
    pub fn load() -> Result<Self> {
        let state_path = workspace_state_path()?;
        if state_path.exists() {
            let content = fs::read_to_string(&state_path)?;
            Ok(serde_json::from_str(&content)?)
        } else {
            Ok(Self {
                current_node: None,
                intent: None,
                merge_parents: None,
                pre_merge_node: None,
            })
        }
    }

    pub fn save(&self) -> Result<()> {
        let state_path = workspace_state_path()?;
        // Ensure parent directory exists (for linked worktrees)
        if let Some(parent) = state_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        fs::write(state_path, content)?;
        Ok(())
    }

    /// Check if there's a merge in progress
    pub fn is_merge_pending(&self) -> bool {
        self.merge_parents.is_some()
    }

    /// Clear merge state
    pub fn clear_merge(&mut self) {
        self.merge_parents = None;
        self.pre_merge_node = None;
    }
}

