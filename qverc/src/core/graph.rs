//! DAG Graph operations
//!
//! Manages the directed acyclic graph of state nodes.

use crate::core::node::{FileEntry, Manifest, Node, NodeStatus, Zone};
use crate::storage::database::Database;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum GraphError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] crate::storage::database::DatabaseError),

    #[error("Node not found: {0}")]
    NodeNotFound(String),

    #[error("Invalid parent: {0}")]
    InvalidParent(String),

    #[error("Cycle detected in graph")]
    CycleDetected,
}

/// Graph manager for DAG operations
pub struct Graph {
    db: Database,
}

impl Graph {
    /// Create a new graph manager
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// Get the underlying database
    pub fn database(&self) -> &Database {
        &self.db
    }

    /// Get a mutable reference to the database
    pub fn database_mut(&mut self) -> &mut Database {
        &mut self.db
    }

    /// Add a new node to the graph
    pub fn add_node(&mut self, node: &Node) -> Result<(), GraphError> {
        // Verify all parents exist
        for parent_id in &node.manifest.parents {
            if !self.db.node_exists(parent_id)? {
                return Err(GraphError::InvalidParent(parent_id.clone()));
            }
        }

        // Insert the node
        self.db.insert_node(node)?;

        Ok(())
    }

    /// Get a node by ID
    pub fn get_node(&self, node_id: &str) -> Result<Node, GraphError> {
        self.db
            .get_node(node_id)?
            .ok_or_else(|| GraphError::NodeNotFound(node_id.to_string()))
    }

    /// Get the manifest for a node
    pub fn get_manifest(&self, node_id: &str) -> Result<Manifest, GraphError> {
        self.db
            .get_manifest(node_id)?
            .ok_or_else(|| GraphError::NodeNotFound(node_id.to_string()))
    }

    /// Get parent nodes
    pub fn get_parents(&self, node_id: &str) -> Result<Vec<String>, GraphError> {
        Ok(self.db.get_parents(node_id)?)
    }

    /// Get child nodes
    pub fn get_children(&self, node_id: &str) -> Result<Vec<String>, GraphError> {
        Ok(self.db.get_children(node_id)?)
    }

    /// Get files for a node
    pub fn get_files(&self, node_id: &str) -> Result<Vec<FileEntry>, GraphError> {
        Ok(self.db.get_files(node_id)?)
    }

    /// Update node status
    pub fn update_status(&mut self, node_id: &str, status: NodeStatus) -> Result<(), GraphError> {
        if !self.db.node_exists(node_id)? {
            return Err(GraphError::NodeNotFound(node_id.to_string()));
        }
        self.db.update_node_status(node_id, status)?;
        Ok(())
    }

    /// Update node zone
    pub fn update_zone(&mut self, node_id: &str, zone: Zone) -> Result<(), GraphError> {
        if !self.db.node_exists(node_id)? {
            return Err(GraphError::NodeNotFound(node_id.to_string()));
        }
        self.db.update_node_zone(node_id, zone)?;
        Ok(())
    }

    /// Get the current HEAD reference
    pub fn get_head(&self) -> Result<Option<String>, GraphError> {
        Ok(self.db.get_ref("HEAD")?)
    }

    /// Set the HEAD reference
    pub fn set_head(&mut self, node_id: &str) -> Result<(), GraphError> {
        if !self.db.node_exists(node_id)? {
            return Err(GraphError::NodeNotFound(node_id.to_string()));
        }
        self.db.set_ref("HEAD", node_id)?;
        Ok(())
    }

    /// Get the spine tip (latest consolidation node)
    pub fn get_spine_tip(&self) -> Result<Option<String>, GraphError> {
        Ok(self.db.get_ref("SPINE")?)
    }

    /// Set the spine tip
    pub fn set_spine_tip(&mut self, node_id: &str) -> Result<(), GraphError> {
        if !self.db.node_exists(node_id)? {
            return Err(GraphError::NodeNotFound(node_id.to_string()));
        }
        self.db.set_ref("SPINE", node_id)?;
        Ok(())
    }

    /// Get recent nodes for the log
    pub fn get_recent_nodes(
        &self,
        limit: usize,
        include_exploration: bool,
    ) -> Result<Vec<Manifest>, GraphError> {
        Ok(self.db.get_recent_nodes(limit, include_exploration)?)
    }

    /// Traverse ancestors (for history)
    pub fn traverse_ancestors(
        &self,
        start_id: &str,
        limit: usize,
    ) -> Result<Vec<String>, GraphError> {
        let mut result = Vec::new();
        let mut queue = vec![start_id.to_string()];
        let mut visited = std::collections::HashSet::new();

        while let Some(node_id) = queue.pop() {
            if visited.contains(&node_id) || result.len() >= limit {
                continue;
            }
            visited.insert(node_id.clone());
            result.push(node_id.clone());

            let parents = self.get_parents(&node_id)?;
            for parent in parents {
                if !visited.contains(&parent) {
                    queue.push(parent);
                }
            }
        }

        Ok(result)
    }

    /// Find orphaned nodes (no children, not HEAD, not SPINE)
    pub fn find_orphaned_nodes(&self) -> Result<Vec<String>, GraphError> {
        Ok(self.db.find_orphaned_nodes()?)
    }

    /// Find failed nodes (draft status, in exploration zone)
    pub fn find_failed_nodes(&self) -> Result<Vec<String>, GraphError> {
        Ok(self.db.find_nodes_by_status(NodeStatus::Draft)?)
    }

    /// Delete a node (for pruning)
    pub fn delete_node(&mut self, node_id: &str) -> Result<(), GraphError> {
        // Don't delete if it has children
        let children = self.get_children(node_id)?;
        if !children.is_empty() {
            return Err(GraphError::InvalidParent(format!(
                "Node {} has children, cannot delete",
                node_id
            )));
        }

        self.db.delete_node(node_id)?;
        Ok(())
    }

    /// Check if a node exists
    pub fn node_exists(&self, node_id: &str) -> Result<bool, GraphError> {
        Ok(self.db.node_exists(node_id)?)
    }

    /// Count total nodes
    pub fn count_nodes(&self) -> Result<usize, GraphError> {
        Ok(self.db.count_nodes()?)
    }

    /// Find the lowest common ancestor of multiple nodes
    /// Returns None if no common ancestor exists (disjoint branches)
    pub fn find_common_ancestor(&self, node_ids: &[String]) -> Result<Option<String>, GraphError> {
        if node_ids.is_empty() {
            return Ok(None);
        }
        if node_ids.len() == 1 {
            return Ok(Some(node_ids[0].clone()));
        }

        // Get all ancestors for each node
        let mut ancestor_sets: Vec<std::collections::HashSet<String>> = Vec::new();
        
        for node_id in node_ids {
            let mut ancestors = std::collections::HashSet::new();
            let mut queue = vec![node_id.clone()];
            
            while let Some(current) = queue.pop() {
                if ancestors.contains(&current) {
                    continue;
                }
                ancestors.insert(current.clone());
                
                let parents = self.get_parents(&current)?;
                for parent in parents {
                    if !ancestors.contains(&parent) {
                        queue.push(parent);
                    }
                }
            }
            
            ancestor_sets.push(ancestors);
        }

        // Find intersection of all ancestor sets
        if ancestor_sets.is_empty() {
            return Ok(None);
        }

        let mut common: std::collections::HashSet<String> = ancestor_sets[0].clone();
        for set in &ancestor_sets[1..] {
            common = common.intersection(set).cloned().collect();
        }

        if common.is_empty() {
            return Ok(None);
        }

        // Find the "lowest" common ancestor (closest to the nodes)
        // This is the one with the highest depth from roots
        let mut best_ancestor: Option<String> = None;
        let mut best_depth = 0;

        for ancestor in &common {
            // Calculate depth by counting max distance from any root
            let depth = self.calculate_depth(ancestor)?;
            if depth > best_depth || best_ancestor.is_none() {
                best_depth = depth;
                best_ancestor = Some(ancestor.clone());
            }
        }

        Ok(best_ancestor)
    }

    /// Calculate the depth of a node (max distance from any root)
    fn calculate_depth(&self, node_id: &str) -> Result<usize, GraphError> {
        let parents = self.get_parents(node_id)?;
        if parents.is_empty() {
            return Ok(0);
        }

        let mut max_parent_depth = 0;
        for parent in parents {
            let parent_depth = self.calculate_depth(&parent)?;
            max_parent_depth = max_parent_depth.max(parent_depth);
        }

        Ok(max_parent_depth + 1)
    }
}

