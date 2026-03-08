//! `qverc merge` command
//!
//! Merges multiple nodes into a new state by combining their files.
//! This is a two-phase operation:
//! 1. `qverc merge` - prepares the workspace with merged content and conflict info
//! 2. `qverc sync` - commits the merge after conflicts are resolved
//!
//! Key design principle: NO auto-choosing of conflicting files.
//! All versions are preserved for the agent to synthesize a solution.

use crate::cli::init::{db_path, find_qvern_root, qvern_dir, WorkspaceState};
use crate::core::config::Config;
use crate::core::graph::Graph;
use crate::core::node::FileEntry;
use crate::storage::cas::ContentStore;
use crate::storage::database::Database;
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Information about a file from a specific node
#[derive(Debug, Clone)]
struct FileInfo {
    node_id: String,
    blob_hash: String,
    mode: u32,
    _node_timestamp: DateTime<Utc>,
}

/// File status in the merge
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FileStatus {
    /// File exists only in one node
    Unique,
    /// File is identical across all nodes
    Identical,
    /// File differs between nodes - requires resolution
    Conflict,
}

/// Information about a parent node in the merge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeParentInfo {
    pub node_id: String,
    pub intent: Option<String>,
    pub agent: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Information about a file in the merge manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeFileInfo {
    pub status: FileStatus,
    /// For unique files, which node it came from
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// For conflicts, list of nodes with different versions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub versions: Option<Vec<String>>,
    /// Whether this conflict has been resolved
    #[serde(default)]
    pub resolved: bool,
}

/// Intent analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentAnalysis {
    /// Whether intents appear compatible
    pub compatible: bool,
    /// Analysis notes
    pub notes: String,
    /// Flagged potential conflicts
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

/// The merge manifest - complete context for the agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeManifest {
    pub merge_id: String,
    pub parent_nodes: Vec<MergeParentInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub common_ancestor: Option<String>,
    pub files: HashMap<String, MergeFileInfo>,
    pub intent_analysis: IntentAnalysis,
    pub created_at: DateTime<Utc>,
}

impl MergeManifest {
    /// Get the path to the merge directory
    pub fn merge_dir() -> Result<PathBuf> {
        Ok(qvern_dir()?.join("merge"))
    }

    /// Get the path to the manifest file
    pub fn manifest_path() -> Result<PathBuf> {
        Ok(Self::merge_dir()?.join("manifest.json"))
    }

    /// Load the merge manifest if it exists
    pub fn load() -> Result<Option<Self>> {
        let path = Self::manifest_path()?;
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)?;
        let manifest: Self = serde_json::from_str(&content)?;
        Ok(Some(manifest))
    }

    /// Save the merge manifest
    pub fn save(&self) -> Result<()> {
        let path = Self::manifest_path()?;
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }

    /// Check if there are unresolved conflicts
    pub fn has_unresolved_conflicts(&self) -> bool {
        self.files.values().any(|f| {
            f.status == FileStatus::Conflict && !f.resolved
        })
    }

    /// Get list of unresolved conflict paths
    pub fn unresolved_conflicts(&self) -> Vec<&String> {
        self.files
            .iter()
            .filter(|(_, f)| f.status == FileStatus::Conflict && !f.resolved)
            .map(|(path, _)| path)
            .collect()
    }

    /// Clean up merge directory
    pub fn cleanup() -> Result<()> {
        let merge_dir = Self::merge_dir()?;
        if merge_dir.exists() {
            fs::remove_dir_all(&merge_dir)?;
        }
        Ok(())
    }
}

/// Run the merge command
pub fn run(node_ids: &[String], intent: Option<&str>) -> Result<()> {
    if node_ids.len() < 2 {
        bail!("Merge requires at least 2 node IDs");
    }

    let root = find_qvern_root()?;
    let qvern = qvern_dir()?;

    // Check for existing merge
    if MergeManifest::load()?.is_some() {
        bail!("A merge is already in progress. Run 'qverc merge --abort' to cancel or 'qverc sync' to complete.");
    }

    // Open graph
    let db = Database::open(db_path()?)?;
    let graph = Graph::new(db);

    // Validate all nodes exist and collect their info
    println!("{} Validating nodes...", "→".blue());
    let mut node_infos: Vec<(String, DateTime<Utc>, Vec<FileEntry>, Option<String>, Option<String>)> = Vec::new();
    
    for node_id in node_ids {
        if !graph.node_exists(node_id)? {
            bail!("Node not found: {}", node_id);
        }
        
        let manifest = graph.get_manifest(node_id)?;
        let files = graph.get_files(node_id)?;
        
        println!(
            "  {} {} ({} files)",
            "✓".green(),
            node_id.cyan(),
            files.len()
        );
        
        if let Some(intent) = &manifest.intent_prompt {
            println!("    Intent: {}", intent.dimmed());
        }
        
        node_infos.push((
            node_id.clone(),
            manifest.created_at,
            files,
            manifest.intent_prompt,
            manifest.agent_signature,
        ));
    }

    // Find common ancestor
    let common_ancestor = graph.find_common_ancestor(node_ids)?;
    if let Some(ref ancestor) = common_ancestor {
        println!(
            "  {} Common ancestor: {}",
            "→".blue(),
            ancestor.cyan()
        );
    } else {
        println!(
            "  {} No common ancestor found (disjoint branches)",
            "→".yellow()
        );
    }

    // Build file map
    println!("{} Analyzing files...", "→".blue());
    let file_map = build_file_map(&node_infos)?;
    
    let mut unique_count = 0;
    let mut identical_count = 0;
    let mut conflict_count = 0;
    let mut merge_files: HashMap<String, MergeFileInfo> = HashMap::new();
    
    for (path, infos) in &file_map {
        if infos.len() == 1 {
            unique_count += 1;
            merge_files.insert(path.clone(), MergeFileInfo {
                status: FileStatus::Unique,
                source: Some(infos[0].node_id.clone()),
                versions: None,
                resolved: true, // Unique files are auto-resolved
            });
        } else {
            // Check if all versions are identical
            let first_hash = &infos[0].blob_hash;
            if infos.iter().all(|f| &f.blob_hash == first_hash) {
                identical_count += 1;
                merge_files.insert(path.clone(), MergeFileInfo {
                    status: FileStatus::Identical,
                    source: None,
                    versions: None,
                    resolved: true, // Identical files are auto-resolved
                });
            } else {
                conflict_count += 1;
                let versions: Vec<String> = infos.iter().map(|f| f.node_id.clone()).collect();
                merge_files.insert(path.clone(), MergeFileInfo {
                    status: FileStatus::Conflict,
                    source: None,
                    versions: Some(versions),
                    resolved: false, // Conflicts need resolution
                });
            }
        }
    }
    
    println!(
        "  {} files total: {} unique, {} identical, {} conflicts",
        file_map.len(),
        unique_count.to_string().green(),
        identical_count.to_string().green(),
        if conflict_count > 0 { 
            conflict_count.to_string().yellow() 
        } else { 
            conflict_count.to_string().green() 
        }
    );

    // Analyze intents
    let intent_analysis = analyze_intents(&node_infos);
    if !intent_analysis.warnings.is_empty() {
        println!("{} Intent analysis:", "→".yellow());
        for warning in &intent_analysis.warnings {
            println!("  {} {}", "⚠".yellow(), warning);
        }
    }

    // Create merge directory structure
    println!("{} Creating merge workspace...", "→".blue());
    let merge_dir = MergeManifest::merge_dir()?;
    fs::create_dir_all(&merge_dir)?;
    fs::create_dir_all(merge_dir.join("files"))?;

    // Build parent info
    let parent_nodes: Vec<MergeParentInfo> = node_infos
        .iter()
        .map(|(node_id, created_at, _, intent, agent)| MergeParentInfo {
            node_id: node_id.clone(),
            intent: intent.clone(),
            agent: agent.clone(),
            created_at: *created_at,
        })
        .collect();

    // Create and save manifest
    let merge_id = format!("merge-{}", &uuid_short());
    let manifest = MergeManifest {
        merge_id: merge_id.clone(),
        parent_nodes,
        common_ancestor: common_ancestor.clone(),
        files: merge_files,
        intent_analysis,
        created_at: Utc::now(),
    };
    manifest.save()?;

    // Write conflict file versions
    let cas = ContentStore::new(&qvern);
    let config = Config::load_from_repo(&root).unwrap_or_default();
    
    write_conflict_versions(&merge_dir, &cas, &file_map, &manifest)?;

    // Write non-conflicting files to workspace
    write_resolved_files(&root, &cas, &file_map, &manifest, &config)?;

    // Generate intents.md
    generate_intents_md(&merge_dir, &manifest, &file_map)?;

    // Update workspace state
    let mut workspace_state = WorkspaceState::load()?;
    
    // Save the current node so we can restore on abort
    workspace_state.pre_merge_node = workspace_state.current_node.clone();
    
    workspace_state.merge_parents = Some(node_ids.to_vec());
    workspace_state.current_node = None; // Clear - we're in a merge state now
    
    if let Some(intent) = intent {
        workspace_state.intent = Some(intent.to_string());
    } else {
        let default_intent = format!(
            "Merge {} nodes: {}",
            node_ids.len(),
            node_ids.join(", ")
        );
        workspace_state.intent = Some(default_intent);
    }
    
    workspace_state.save()?;

    // Summary
    println!();
    println!(
        "{} Merge prepared: {}",
        "success:".green().bold(),
        merge_id.cyan()
    );
    println!("  Parents: {}", node_ids.iter().map(|s| s.cyan().to_string()).collect::<Vec<_>>().join(", "));
    
    if conflict_count > 0 {
        println!();
        println!("{}", "Conflicts requiring resolution:".yellow().bold());
        for (path, info) in manifest.files.iter().filter(|(_, f)| f.status == FileStatus::Conflict) {
            println!("  {} {}", "•".yellow(), path);
            if let Some(versions) = &info.versions {
                for node_id in versions {
                    println!("    └─ {}", node_id.dimmed());
                }
            }
        }
        println!();
        println!("Conflict files stored in: {}", ".qverc/merge/files/".cyan());
        println!("Intent summary available: {}", ".qverc/merge/intents.md".cyan());
        println!();
        println!("Next steps:");
        println!("  1. Review {} for full context", ".qverc/merge/intents.md".cyan());
        println!("  2. For each conflict, examine versions in {}", ".qverc/merge/files/".cyan());
        println!("  3. Create merged version in the workspace");
        println!("  4. Run {} when ready", "qverc sync".cyan());
    } else {
        println!();
        println!("No conflicts! All files are either unique or identical.");
        println!("Run {} to complete the merge.", "qverc sync".cyan());
    }

    Ok(())
}

/// Build a map of path -> [FileInfo] from all nodes
fn build_file_map(
    node_infos: &[(String, DateTime<Utc>, Vec<FileEntry>, Option<String>, Option<String>)],
) -> Result<HashMap<String, Vec<FileInfo>>> {
    let mut file_map: HashMap<String, Vec<FileInfo>> = HashMap::new();

    for (node_id, timestamp, files, _, _) in node_infos {
        for file in files {
            let info = FileInfo {
                node_id: node_id.clone(),
                blob_hash: file.blob_hash.clone(),
                mode: file.mode,
                _node_timestamp: *timestamp,
            };
            
            file_map
                .entry(file.path.clone())
                .or_default()
                .push(info);
        }
    }

    Ok(file_map)
}

/// Analyze intents for potential conflicts
fn analyze_intents(
    node_infos: &[(String, DateTime<Utc>, Vec<FileEntry>, Option<String>, Option<String>)],
) -> IntentAnalysis {
    let intents: Vec<&str> = node_infos
        .iter()
        .filter_map(|(_, _, _, intent, _)| intent.as_deref())
        .collect();

    let mut warnings = Vec::new();
    let mut notes = String::new();

    if intents.is_empty() {
        notes = "No intents specified in parent nodes".to_string();
        return IntentAnalysis {
            compatible: true,
            notes,
            warnings,
        };
    }

    // Simple heuristic checks for potentially conflicting intents
    let intent_lower: Vec<String> = intents.iter().map(|s| s.to_lowercase()).collect();
    
    // Check for opposing keywords
    let opposing_pairs = [
        ("add", "remove"),
        ("enable", "disable"),
        ("show", "hide"),
        ("increase", "decrease"),
        ("light", "dark"),
        ("modern", "classic"),
        ("minimal", "detailed"),
    ];

    for (word1, word2) in opposing_pairs {
        let has_word1 = intent_lower.iter().any(|i| i.contains(word1));
        let has_word2 = intent_lower.iter().any(|i| i.contains(word2));
        
        if has_word1 && has_word2 {
            warnings.push(format!(
                "Potentially opposing intents detected: '{}' vs '{}'",
                word1, word2
            ));
        }
    }

    // Check if intents seem to target the same component
    let components: Vec<HashSet<&str>> = intent_lower
        .iter()
        .map(|i| {
            i.split_whitespace()
                .filter(|w| w.len() > 3)
                .collect()
        })
        .collect();

    if components.len() >= 2 {
        for i in 0..components.len() {
            for j in (i + 1)..components.len() {
                let overlap: HashSet<_> = components[i].intersection(&components[j]).collect();
                if overlap.len() >= 2 {
                    // Multiple overlapping words might indicate same-area changes
                    let overlap_words: Vec<_> = overlap.iter().map(|s| **s).collect();
                    if !warnings.iter().any(|w| w.contains("same area")) {
                        warnings.push(format!(
                            "Multiple intents may affect the same area (shared terms: {})",
                            overlap_words.join(", ")
                        ));
                    }
                }
            }
        }
    }

    let compatible = warnings.is_empty();
    notes = if compatible {
        "Intents appear compatible - additive or orthogonal changes".to_string()
    } else {
        "Review flagged warnings - may require careful synthesis".to_string()
    };

    IntentAnalysis {
        compatible,
        notes,
        warnings,
    }
}

/// Write all versions of conflicting files to the merge directory
fn write_conflict_versions(
    merge_dir: &Path,
    cas: &ContentStore,
    file_map: &HashMap<String, Vec<FileInfo>>,
    manifest: &MergeManifest,
) -> Result<()> {
    let files_dir = merge_dir.join("files");

    for (path, info) in &manifest.files {
        if info.status != FileStatus::Conflict {
            continue;
        }

        let infos = file_map.get(path).unwrap();
        
        // Create directory for this file's versions
        let file_versions_dir = files_dir.join(path);
        fs::create_dir_all(&file_versions_dir)?;

        // Write each version
        for file_info in infos {
            let content = cas
                .retrieve(&file_info.blob_hash)
                .context(format!("Failed to retrieve blob for {}", path))?;

            // Use node ID as filename prefix, keep original extension
            let ext = Path::new(path)
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default();
            
            let version_filename = format!("{}{}", file_info.node_id, ext);
            let version_path = file_versions_dir.join(&version_filename);
            
            fs::write(&version_path, &content)?;

            // Also write a metadata file
            let meta = serde_json::json!({
                "node_id": file_info.node_id,
                "blob_hash": file_info.blob_hash,
                "mode": file_info.mode,
            });
            let meta_path = file_versions_dir.join(format!("{}.meta.json", file_info.node_id));
            fs::write(meta_path, serde_json::to_string_pretty(&meta)?)?;
        }

        // Write a versions.json summary
        let versions_info: Vec<serde_json::Value> = infos
            .iter()
            .map(|f| {
                let node_intent = manifest.parent_nodes
                    .iter()
                    .find(|p| p.node_id == f.node_id)
                    .and_then(|p| p.intent.clone());
                
                serde_json::json!({
                    "node_id": f.node_id,
                    "intent": node_intent,
                    "blob_hash": f.blob_hash,
                })
            })
            .collect();

        let versions_json = serde_json::json!({
            "path": path,
            "status": "conflict",
            "versions": versions_info,
        });
        
        fs::write(
            file_versions_dir.join("versions.json"),
            serde_json::to_string_pretty(&versions_json)?,
        )?;
    }

    Ok(())
}

/// Write non-conflicting files (unique and identical) to the workspace
fn write_resolved_files(
    root: &Path,
    cas: &ContentStore,
    file_map: &HashMap<String, Vec<FileInfo>>,
    manifest: &MergeManifest,
    config: &Config,
) -> Result<()> {
    let ignore_patterns: HashSet<_> = config.workspace.ignore.iter().collect();

    for (path, info) in &manifest.files {
        // Skip conflicts - agent must resolve these
        if info.status == FileStatus::Conflict {
            continue;
        }

        // Skip ignored paths
        let should_ignore = ignore_patterns.iter().any(|pattern| {
            let pattern = pattern.trim_end_matches('/').trim_end_matches("/**");
            path.starts_with(pattern) || path.as_str() == pattern
        });
        
        if should_ignore {
            continue;
        }

        let infos = file_map.get(path).unwrap();
        let chosen = &infos[0]; // For unique/identical, any version works

        let file_path = root.join(path);
        
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = cas
            .retrieve(&chosen.blob_hash)
            .context(format!("Failed to retrieve blob for {}", path))?;

        fs::write(&file_path, content)?;

        let permissions = std::fs::Permissions::from_mode(chosen.mode);
        fs::set_permissions(&file_path, permissions)?;
    }

    Ok(())
}

/// Generate the intents.md agent-readable summary
fn generate_intents_md(
    merge_dir: &Path,
    manifest: &MergeManifest,
    file_map: &HashMap<String, Vec<FileInfo>>,
) -> Result<()> {
    let mut md = String::new();

    md.push_str(&format!("# Merge: {}\n\n", manifest.merge_id));

    // Parent intents section
    md.push_str("## Parent Node Intents\n\n");
    for parent in &manifest.parent_nodes {
        md.push_str(&format!("### {}\n", parent.node_id));
        if let Some(intent) = &parent.intent {
            md.push_str(&format!("> {}\n", intent));
        } else {
            md.push_str("> *(no intent specified)*\n");
        }
        if let Some(agent) = &parent.agent {
            md.push_str(&format!("\n*Agent: {}*\n", agent));
        }
        md.push_str(&format!("*Created: {}*\n\n", parent.created_at.format("%Y-%m-%d %H:%M:%S UTC")));
    }

    // Common ancestor
    if let Some(ancestor) = &manifest.common_ancestor {
        md.push_str(&format!("## Common Ancestor\n\n`{}`\n\n", ancestor));
    } else {
        md.push_str("## Common Ancestor\n\n*No common ancestor (disjoint branches)*\n\n");
    }

    // Intent analysis
    md.push_str("## Intent Analysis\n\n");
    if manifest.intent_analysis.compatible {
        md.push_str("✅ **Intents appear compatible**\n\n");
    } else {
        md.push_str("⚠️ **Review recommended**\n\n");
    }
    md.push_str(&format!("{}\n\n", manifest.intent_analysis.notes));
    
    if !manifest.intent_analysis.warnings.is_empty() {
        md.push_str("### Warnings\n\n");
        for warning in &manifest.intent_analysis.warnings {
            md.push_str(&format!("- ⚠️ {}\n", warning));
        }
        md.push_str("\n");
    }

    // Conflicts section
    let conflicts: Vec<_> = manifest.files
        .iter()
        .filter(|(_, f)| f.status == FileStatus::Conflict)
        .collect();

    if !conflicts.is_empty() {
        md.push_str("## Conflicts Requiring Resolution\n\n");
        md.push_str("These files differ between nodes. Review all versions and create a merged solution.\n\n");

        for (path, info) in &conflicts {
            md.push_str(&format!("### `{}`\n\n", path));
            md.push_str("| Node | Intent | Version File |\n");
            md.push_str("|------|--------|-------------|\n");
            
            if let Some(versions) = &info.versions {
                for node_id in versions {
                    let intent = manifest.parent_nodes
                        .iter()
                        .find(|p| &p.node_id == node_id)
                        .and_then(|p| p.intent.as_deref())
                        .unwrap_or("*(none)*");
                    
                    let ext = Path::new(path.as_str())
                        .extension()
                        .map(|e| format!(".{}", e.to_string_lossy()))
                        .unwrap_or_default();
                    
                    let version_file = format!(".qverc/merge/files/{}/{}{}", path, node_id, ext);
                    
                    md.push_str(&format!("| `{}` | {} | `{}` |\n", node_id, intent, version_file));
                }
            }
            md.push_str("\n");
        }
    }

    // Unique files section
    let unique: Vec<_> = manifest.files
        .iter()
        .filter(|(_, f)| f.status == FileStatus::Unique)
        .collect();

    if !unique.is_empty() {
        md.push_str("## Unique Files (Auto-included)\n\n");
        md.push_str("These files exist only in one parent node and are automatically included.\n\n");
        
        for (path, info) in &unique {
            let source = info.source.as_deref().unwrap_or("unknown");
            md.push_str(&format!("- `{}` ← `{}`\n", path, source));
        }
        md.push_str("\n");
    }

    // Identical files section
    let identical_count = manifest.files
        .iter()
        .filter(|(_, f)| f.status == FileStatus::Identical)
        .count();

    if identical_count > 0 {
        md.push_str(&format!("## Identical Files\n\n{} files are identical across all parent nodes.\n\n", identical_count));
    }

    // Instructions
    md.push_str("---\n\n");
    md.push_str("## Resolution Instructions\n\n");
    md.push_str("1. **Review each conflict** in `.qverc/merge/files/<path>/`\n");
    md.push_str("2. **Understand the intent** of each version from the table above\n");
    md.push_str("3. **Create the merged file** in the workspace that preserves functionality from ALL versions\n");
    md.push_str("4. **Consider creative solutions** - if intents seem incompatible, you might implement both (e.g., feature flags, theme selector)\n");
    md.push_str("5. Run `qverc sync` when all conflicts are resolved\n");

    fs::write(merge_dir.join("intents.md"), md)?;

    Ok(())
}

/// Generate a short UUID-like identifier
fn uuid_short() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{:x}", now % 0xFFFFFF)
}

/// Show current merge status
pub fn status() -> Result<()> {
    let manifest = MergeManifest::load()?;
    
    match manifest {
        None => {
            println!("No merge in progress.");
        }
        Some(m) => {
            println!("{} Merge in progress: {}", "→".blue(), m.merge_id.cyan());
            println!();
            
            println!("Parent nodes:");
            for parent in &m.parent_nodes {
                println!("  {} {}", "•".blue(), parent.node_id.cyan());
                if let Some(intent) = &parent.intent {
                    println!("    {}", intent.dimmed());
                }
            }
            println!();

            let unresolved = m.unresolved_conflicts();
            if unresolved.is_empty() {
                println!("{} All conflicts resolved!", "✓".green());
                println!("Run {} to complete the merge.", "qverc sync".cyan());
            } else {
                println!("{} {} unresolved conflicts:", "⚠".yellow(), unresolved.len());
                for path in unresolved {
                    println!("  {} {}", "•".yellow(), path);
                }
                println!();
                println!("Review files in {} and create merged versions.", ".qverc/merge/files/".cyan());
            }
        }
    }

    Ok(())
}

/// Abort a pending merge
pub fn abort() -> Result<()> {
    let manifest = MergeManifest::load()?;
    
    if manifest.is_none() {
        bail!("No merge in progress");
    }
    
    // Get the pre-merge node before clearing state
    let workspace_state = WorkspaceState::load()?;
    let restore_node = workspace_state.pre_merge_node.clone();
    
    // Clean up merge directory
    MergeManifest::cleanup()?;
    
    // Restore to the pre-merge state if we have one
    if let Some(node_id) = restore_node {
        println!("{} Restoring workspace to {}...", "→".blue(), node_id.cyan());
        
        // Use checkout to restore the workspace (force=true since we're aborting)
        crate::cli::checkout::run(&node_id, true)?;
        
        println!("{} Merge aborted and workspace restored", "success:".green().bold());
    } else {
        // No pre-merge node, just clear the state
        let mut ws = WorkspaceState::load()?;
        ws.clear_merge();
        ws.save()?;
        
        println!("{} Merge aborted", "success:".green().bold());
        println!("  No previous node to restore to.");
        println!("  Use {} to checkout a node.", "qverc checkout".cyan());
    }
    
    Ok(())
}

/// Mark a conflict as resolved (for use by sync to detect resolution)
pub fn mark_resolved(path: &str) -> Result<()> {
    let mut manifest = MergeManifest::load()?
        .ok_or_else(|| anyhow::anyhow!("No merge in progress"))?;
    
    if let Some(file_info) = manifest.files.get_mut(path) {
        if file_info.status == FileStatus::Conflict {
            file_info.resolved = true;
            manifest.save()?;
            println!("{} Marked as resolved: {}", "✓".green(), path);
        }
    } else {
        bail!("File not found in merge: {}", path);
    }

    Ok(())
}
