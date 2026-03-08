//! `qverc worktree` command
//!
//! Manage multiple working directories connected to the same DAG.

use crate::cli::init::{get_worktree_info, WorkspaceState, WorktreeLink};
use crate::storage::database::Database;
use anyhow::{bail, Context, Result};
use chrono::Utc;
use colored::Colorize;
use std::fs;
use std::path::{Path, PathBuf};

/// Worktree metadata stored in .qverc/worktrees/<name>/
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct WorktreeMetadata {
    /// Absolute path to the worktree directory
    pub worktree_path: String,
    /// When this worktree was created
    pub created_at: String,
}

/// Run the worktree add command
pub fn run_add(path: &str, node_id: Option<&str>, name: Option<&str>) -> Result<()> {
    let info = get_worktree_info()?;
    
    if info.is_linked {
        bail!("Cannot create worktree from within a linked worktree. Run from the main worktree.");
    }
    
    // Resolve target path
    let target_path = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        std::env::current_dir()?.join(path)
    };
    
    let target_path = target_path.canonicalize().unwrap_or(target_path);
    
    // Determine worktree name
    let worktree_name = name.map(String::from).unwrap_or_else(|| {
        target_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| format!("worktree-{}", Utc::now().timestamp()))
    });
    
    // Check if worktree name already exists
    let worktrees_dir = info.main_repo.join("worktrees");
    let worktree_meta_dir = worktrees_dir.join(&worktree_name);
    if worktree_meta_dir.exists() {
        bail!("Worktree '{}' already exists", worktree_name);
    }
    
    // Check if target path already exists and is not empty
    if target_path.exists() {
        let entries: Vec<_> = fs::read_dir(&target_path)?.collect();
        if !entries.is_empty() {
            bail!(
                "Target directory '{}' is not empty",
                target_path.display()
            );
        }
    }
    
    // Determine which node to check out
    let checkout_node = match node_id {
        Some(id) => id.to_string(),
        None => {
            // Use current HEAD
            let state = WorkspaceState::load()?;
            state.current_node.ok_or_else(|| {
                anyhow::anyhow!("No current node. Specify a node ID to check out.")
            })?
        }
    };
    
    // Verify node exists
    let db_path = info.main_repo.join("db.sqlite");
    let db = Database::open(&db_path)?;
    if !db.node_exists(&checkout_node)? {
        bail!("Node '{}' does not exist", checkout_node);
    }
    
    println!(
        "{} Creating worktree '{}' at {}",
        "→".blue(),
        worktree_name.cyan(),
        target_path.display()
    );
    
    // Create target directory
    fs::create_dir_all(&target_path)
        .context("Failed to create worktree directory")?;
    
    // Create .qverc link file in worktree
    let link = WorktreeLink {
        main_repo: info.main_repo.to_string_lossy().to_string(),
        worktree_name: worktree_name.clone(),
    };
    let link_path = target_path.join(".qverc");
    let link_content = serde_json::to_string_pretty(&link)?;
    fs::write(&link_path, link_content)
        .context("Failed to write .qverc link file")?;
    
    // Create worktree metadata directory
    fs::create_dir_all(&worktree_meta_dir)
        .context("Failed to create worktree metadata directory")?;
    
    // Write worktree metadata
    let metadata = WorktreeMetadata {
        worktree_path: target_path.to_string_lossy().to_string(),
        created_at: Utc::now().to_rfc3339(),
    };
    let metadata_path = worktree_meta_dir.join("metadata.json");
    fs::write(&metadata_path, serde_json::to_string_pretty(&metadata)?)
        .context("Failed to write worktree metadata")?;
    
    // Create initial workspace state for the worktree
    let worktree_state = WorkspaceState {
        current_node: Some(checkout_node.clone()),
        intent: None,
        merge_parents: None,
        pre_merge_node: None,
    };
    let state_path = worktree_meta_dir.join("workspace.json");
    fs::write(&state_path, serde_json::to_string_pretty(&worktree_state)?)
        .context("Failed to write worktree state")?;
    
    // Checkout the node in the worktree
    println!("  Checking out node {}...", checkout_node.cyan());
    
    // We need to checkout files to the worktree directory
    // Get files from the node and write them
    if let Some(node) = db.get_node(&checkout_node)? {
        let cas = crate::storage::cas::ContentStore::new(&info.main_repo);
        
        for file in &node.files {
            let file_path = target_path.join(&file.path);
            
            // Create parent directories
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent)?;
            }
            
            // Read blob and write to file
            let content = cas.retrieve(&file.blob_hash)?;
            fs::write(&file_path, content)?;
            
            // Set file permissions on Unix
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = std::fs::Permissions::from_mode(file.mode);
                fs::set_permissions(&file_path, perms)?;
            }
        }
        
        println!("  Wrote {} files", node.files.len());
    }
    
    // Copy qverc.toml if it exists in main worktree
    let main_config = info.worktree_root.join("qverc.toml");
    let target_config = target_path.join("qverc.toml");
    if main_config.exists() && !target_config.exists() {
        fs::copy(&main_config, &target_config)?;
        println!("  Copied qverc.toml");
    }
    
    println!();
    println!(
        "{} Worktree created at {}",
        "success:".green().bold(),
        target_path.display()
    );
    println!("  Node: {}", checkout_node.cyan());
    println!();
    println!("To use this worktree:");
    println!("  cd {}", target_path.display());
    
    Ok(())
}

/// Run the worktree list command
pub fn run_list() -> Result<()> {
    let info = get_worktree_info()?;
    
    // Get main repo path (even if we're in a linked worktree)
    let main_repo = if info.is_linked {
        info.main_repo.clone()
    } else {
        info.main_repo.clone()
    };
    
    println!("{}", "qverc worktrees".bold());
    println!();
    
    // Show main worktree
    let main_state_path = main_repo.join("workspace.json");
    let main_state: Option<WorkspaceState> = if main_state_path.exists() {
        let content = fs::read_to_string(&main_state_path)?;
        serde_json::from_str(&content).ok()
    } else {
        None
    };
    
    // Find main worktree root (parent of .qverc)
    let main_root = main_repo.parent().unwrap_or(&main_repo);
    
    let main_node = main_state
        .as_ref()
        .and_then(|s| s.current_node.as_ref())
        .map(|s| s.as_str())
        .unwrap_or("(none)");
    
    let main_marker = if !info.is_linked { " (current)" } else { "" };
    
    println!(
        "  {} {}{}",
        main_root.display().to_string().cyan(),
        main_node,
        main_marker.yellow()
    );
    
    // Show linked worktrees
    let worktrees_dir = main_repo.join("worktrees");
    if worktrees_dir.exists() {
        for entry in fs::read_dir(&worktrees_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let wt_name = entry.file_name().to_string_lossy().to_string();
                
                // Read metadata
                let meta_path = entry.path().join("metadata.json");
                let metadata: Option<WorktreeMetadata> = if meta_path.exists() {
                    let content = fs::read_to_string(&meta_path)?;
                    serde_json::from_str(&content).ok()
                } else {
                    None
                };
                
                // Read state
                let state_path = entry.path().join("workspace.json");
                let state: Option<WorkspaceState> = if state_path.exists() {
                    let content = fs::read_to_string(&state_path)?;
                    serde_json::from_str(&content).ok()
                } else {
                    None
                };
                
                let wt_path = metadata
                    .as_ref()
                    .map(|m| m.worktree_path.as_str())
                    .unwrap_or("(unknown)");
                
                let wt_node = state
                    .as_ref()
                    .and_then(|s| s.current_node.as_ref())
                    .map(|s| s.as_str())
                    .unwrap_or("(none)");
                
                let current_marker = if info.is_linked && info.worktree_name == wt_name {
                    " (current)"
                } else {
                    ""
                };
                
                // Check if worktree still exists
                let exists = Path::new(wt_path).exists();
                let status = if exists { "" } else { " [missing]" };
                
                println!(
                    "  {} {}{}{}",
                    wt_path.cyan(),
                    wt_node,
                    current_marker.yellow(),
                    status.red()
                );
            }
        }
    }
    
    Ok(())
}

/// Run the worktree remove command
pub fn run_remove(path: &str, force: bool) -> Result<()> {
    let info = get_worktree_info()?;
    
    let main_repo = if info.is_linked {
        info.main_repo.clone()
    } else {
        info.main_repo.clone()
    };
    
    // Resolve the path
    let target_path = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        std::env::current_dir()?.join(path)
    };
    
    let target_path = target_path.canonicalize().unwrap_or(target_path);
    
    // Find the worktree by path
    let worktrees_dir = main_repo.join("worktrees");
    let mut found_worktree: Option<String> = None;
    
    if worktrees_dir.exists() {
        for entry in fs::read_dir(&worktrees_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let wt_name = entry.file_name().to_string_lossy().to_string();
                let meta_path = entry.path().join("metadata.json");
                
                if meta_path.exists() {
                    let content = fs::read_to_string(&meta_path)?;
                    if let Ok(metadata) = serde_json::from_str::<WorktreeMetadata>(&content) {
                        let wt_path = PathBuf::from(&metadata.worktree_path);
                        let wt_path = wt_path.canonicalize().unwrap_or(wt_path);
                        
                        if wt_path == target_path {
                            found_worktree = Some(wt_name);
                            break;
                        }
                    }
                }
            }
        }
    }
    
    let worktree_name = found_worktree.ok_or_else(|| {
        anyhow::anyhow!("No worktree found at '{}'", target_path.display())
    })?;
    
    // Check if we're IN the worktree we're trying to remove
    if info.is_linked && info.worktree_name == worktree_name {
        bail!("Cannot remove current worktree. cd to another directory first.");
    }
    
    println!(
        "{} Removing worktree '{}' at {}",
        "→".blue(),
        worktree_name.cyan(),
        target_path.display()
    );
    
    // Remove the worktree directory (if it exists and force is set, or if empty)
    if target_path.exists() {
        if force {
            fs::remove_dir_all(&target_path)
                .context("Failed to remove worktree directory")?;
            println!("  Removed worktree directory");
        } else {
            // Check for modifications
            // For now, just try to remove (will fail if not empty with important files)
            if let Err(_) = fs::remove_dir(&target_path) {
                bail!(
                    "Worktree directory is not empty. Use --force to remove anyway."
                );
            }
            println!("  Removed worktree directory");
        }
    }
    
    // Remove metadata
    let meta_dir = worktrees_dir.join(&worktree_name);
    if meta_dir.exists() {
        fs::remove_dir_all(&meta_dir)
            .context("Failed to remove worktree metadata")?;
        println!("  Removed worktree metadata");
    }
    
    println!();
    println!("{} Worktree '{}' removed", "success:".green().bold(), worktree_name);
    
    Ok(())
}

/// Prune worktree metadata for deleted worktrees
pub fn run_prune() -> Result<()> {
    let info = get_worktree_info()?;
    
    let main_repo = if info.is_linked {
        info.main_repo.clone()
    } else {
        info.main_repo.clone()
    };
    
    let worktrees_dir = main_repo.join("worktrees");
    if !worktrees_dir.exists() {
        println!("No worktrees to prune");
        return Ok(());
    }
    
    let mut pruned = 0;
    
    for entry in fs::read_dir(&worktrees_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let wt_name = entry.file_name().to_string_lossy().to_string();
            let meta_path = entry.path().join("metadata.json");
            
            if meta_path.exists() {
                let content = fs::read_to_string(&meta_path)?;
                if let Ok(metadata) = serde_json::from_str::<WorktreeMetadata>(&content) {
                    let wt_path = PathBuf::from(&metadata.worktree_path);
                    
                    if !wt_path.exists() {
                        println!("  Pruning '{}' (directory missing)", wt_name);
                        fs::remove_dir_all(entry.path())?;
                        pruned += 1;
                    }
                }
            }
        }
    }
    
    if pruned > 0 {
        println!();
        println!("{} Pruned {} worktree(s)", "success:".green().bold(), pruned);
    } else {
        println!("No worktrees to prune");
    }
    
    Ok(())
}
