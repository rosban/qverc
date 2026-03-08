//! SQLite database layer for the DAG

use crate::core::node::{FileEntry, Manifest, Metrics, Node, NodeStatus, Zone};
use chrono::{TimeZone, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DatabaseError {
    #[error("SQLite error: {0}")]
    SqliteError(#[from] rusqlite::Error),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Database not initialized")]
    NotInitialized,
}

/// SQLite database for the DAG
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open or create a database at the given path
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DatabaseError> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")?;
        Ok(Self { conn })
    }

    /// Create an in-memory database (for testing)
    pub fn open_in_memory() -> Result<Self, DatabaseError> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    /// Initialize the database schema
    pub fn init_schema(&self) -> Result<(), DatabaseError> {
        self.conn.execute_batch(
            r#"
            -- Nodes table (State Nodes with Manifests)
            CREATE TABLE IF NOT EXISTS nodes (
                node_id TEXT PRIMARY KEY,
                status TEXT NOT NULL,
                zone TEXT NOT NULL,
                intent_prompt TEXT,
                agent_signature TEXT,
                metrics_json TEXT,
                failure_history_json TEXT,
                created_at INTEGER NOT NULL,
                tree_hash TEXT NOT NULL
            );

            -- Parent-child relationships (DAG edges)
            CREATE TABLE IF NOT EXISTS edges (
                child_id TEXT NOT NULL,
                parent_id TEXT NOT NULL,
                PRIMARY KEY (child_id, parent_id),
                FOREIGN KEY (child_id) REFERENCES nodes(node_id) ON DELETE CASCADE,
                FOREIGN KEY (parent_id) REFERENCES nodes(node_id)
            );

            -- File entries per node
            CREATE TABLE IF NOT EXISTS files (
                node_id TEXT NOT NULL,
                path TEXT NOT NULL,
                blob_hash TEXT NOT NULL,
                mode INTEGER NOT NULL,
                PRIMARY KEY (node_id, path),
                FOREIGN KEY (node_id) REFERENCES nodes(node_id) ON DELETE CASCADE
            );

            -- HEAD and other refs tracking
            CREATE TABLE IF NOT EXISTS refs (
                name TEXT PRIMARY KEY,
                node_id TEXT NOT NULL
            );

            -- Indexes for common queries
            CREATE INDEX IF NOT EXISTS idx_nodes_status ON nodes(status);
            CREATE INDEX IF NOT EXISTS idx_nodes_zone ON nodes(zone);
            CREATE INDEX IF NOT EXISTS idx_nodes_created ON nodes(created_at);
            CREATE INDEX IF NOT EXISTS idx_edges_parent ON edges(parent_id);
            CREATE INDEX IF NOT EXISTS idx_files_blob ON files(blob_hash);

            -- Enable foreign keys
            PRAGMA foreign_keys = ON;
            "#,
        )?;
        Ok(())
    }

    /// Insert a new node
    pub fn insert_node(&mut self, node: &Node) -> Result<(), DatabaseError> {
        let tx = self.conn.transaction()?;

        // Insert the node manifest
        tx.execute(
            r#"
            INSERT INTO nodes (
                node_id, status, zone, intent_prompt, agent_signature,
                metrics_json, failure_history_json, created_at, tree_hash
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
            params![
                node.manifest.node_id,
                node.manifest.status.as_str(),
                node.manifest.zone.as_str(),
                node.manifest.intent_prompt,
                node.manifest.agent_signature,
                serde_json::to_string(&node.manifest.metrics)?,
                serde_json::to_string(&node.manifest.failure_history)?,
                node.manifest.created_at.timestamp(),
                node.manifest.tree_hash,
            ],
        )?;

        // Insert parent edges
        for parent_id in &node.manifest.parents {
            tx.execute(
                "INSERT INTO edges (child_id, parent_id) VALUES (?1, ?2)",
                params![node.manifest.node_id, parent_id],
            )?;
        }

        // Insert file entries
        for file in &node.files {
            tx.execute(
                "INSERT INTO files (node_id, path, blob_hash, mode) VALUES (?1, ?2, ?3, ?4)",
                params![node.manifest.node_id, file.path, file.blob_hash, file.mode],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Check if a node exists
    pub fn node_exists(&self, node_id: &str) -> Result<bool, DatabaseError> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM nodes WHERE node_id = ?1",
            params![node_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Get a node by ID
    pub fn get_node(&self, node_id: &str) -> Result<Option<Node>, DatabaseError> {
        let manifest = match self.get_manifest(node_id)? {
            Some(m) => m,
            None => return Ok(None),
        };

        let files = self.get_files(node_id)?;

        Ok(Some(Node { manifest, files }))
    }

    /// Get manifest by ID
    pub fn get_manifest(&self, node_id: &str) -> Result<Option<Manifest>, DatabaseError> {
        let result = self
            .conn
            .query_row(
                r#"
                SELECT node_id, status, zone, intent_prompt, agent_signature,
                       metrics_json, failure_history_json, created_at, tree_hash
                FROM nodes WHERE node_id = ?1
                "#,
                params![node_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, i64>(7)?,
                        row.get::<_, String>(8)?,
                    ))
                },
            )
            .optional()?;

        let (
            node_id,
            status_str,
            zone_str,
            intent_prompt,
            agent_signature,
            metrics_json,
            failure_json,
            created_at_ts,
            tree_hash,
        ) = match result {
            Some(r) => r,
            None => return Ok(None),
        };

        let parents = self.get_parents(&node_id)?;

        let metrics: Metrics = metrics_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?
            .unwrap_or_default();

        let failure_history = failure_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?
            .unwrap_or_default();

        Ok(Some(Manifest {
            node_id,
            parents,
            intent_prompt,
            agent_signature,
            status: NodeStatus::from_str(&status_str).unwrap_or(NodeStatus::Draft),
            zone: Zone::from_str(&zone_str).unwrap_or(Zone::Exploration),
            metrics,
            failure_history,
            created_at: Utc.timestamp_opt(created_at_ts, 0).unwrap(),
            tree_hash,
        }))
    }

    /// Get files for a node
    pub fn get_files(&self, node_id: &str) -> Result<Vec<FileEntry>, DatabaseError> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, blob_hash, mode FROM files WHERE node_id = ?1")?;

        let files = stmt
            .query_map(params![node_id], |row| {
                Ok(FileEntry {
                    path: row.get(0)?,
                    blob_hash: row.get(1)?,
                    mode: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(files)
    }

    /// Get parent node IDs
    pub fn get_parents(&self, node_id: &str) -> Result<Vec<String>, DatabaseError> {
        let mut stmt = self
            .conn
            .prepare("SELECT parent_id FROM edges WHERE child_id = ?1")?;

        let parents = stmt
            .query_map(params![node_id], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;

        Ok(parents)
    }

    /// Get child node IDs
    pub fn get_children(&self, node_id: &str) -> Result<Vec<String>, DatabaseError> {
        let mut stmt = self
            .conn
            .prepare("SELECT child_id FROM edges WHERE parent_id = ?1")?;

        let children = stmt
            .query_map(params![node_id], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;

        Ok(children)
    }

    /// Update node status
    pub fn update_node_status(
        &mut self,
        node_id: &str,
        status: NodeStatus,
    ) -> Result<(), DatabaseError> {
        self.conn.execute(
            "UPDATE nodes SET status = ?1 WHERE node_id = ?2",
            params![status.as_str(), node_id],
        )?;
        Ok(())
    }

    /// Update node zone
    pub fn update_node_zone(&mut self, node_id: &str, zone: Zone) -> Result<(), DatabaseError> {
        self.conn.execute(
            "UPDATE nodes SET zone = ?1 WHERE node_id = ?2",
            params![zone.as_str(), node_id],
        )?;
        Ok(())
    }

    /// Get a reference
    pub fn get_ref(&self, name: &str) -> Result<Option<String>, DatabaseError> {
        let result = self
            .conn
            .query_row(
                "SELECT node_id FROM refs WHERE name = ?1",
                params![name],
                |row| row.get(0),
            )
            .optional()?;
        Ok(result)
    }

    /// Set a reference
    pub fn set_ref(&mut self, name: &str, node_id: &str) -> Result<(), DatabaseError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO refs (name, node_id) VALUES (?1, ?2)",
            params![name, node_id],
        )?;
        Ok(())
    }

    /// Delete a reference
    pub fn delete_ref(&mut self, name: &str) -> Result<(), DatabaseError> {
        self.conn
            .execute("DELETE FROM refs WHERE name = ?1", params![name])?;
        Ok(())
    }

    /// Get recent nodes
    pub fn get_recent_nodes(
        &self,
        limit: usize,
        include_exploration: bool,
    ) -> Result<Vec<Manifest>, DatabaseError> {
        let query = if include_exploration {
            "SELECT node_id FROM nodes ORDER BY created_at DESC LIMIT ?1"
        } else {
            "SELECT node_id FROM nodes WHERE zone = 'consolidation' ORDER BY created_at DESC LIMIT ?1"
        };

        let mut stmt = self.conn.prepare(query)?;
        let node_ids: Vec<String> = stmt
            .query_map(params![limit as i64], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;

        let mut manifests = Vec::new();
        for node_id in node_ids {
            if let Some(manifest) = self.get_manifest(&node_id)? {
                manifests.push(manifest);
            }
        }

        Ok(manifests)
    }

    /// Find orphaned nodes (no children, not a ref target)
    pub fn find_orphaned_nodes(&self) -> Result<Vec<String>, DatabaseError> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT n.node_id FROM nodes n
            WHERE n.zone = 'exploration'
            AND NOT EXISTS (SELECT 1 FROM edges e WHERE e.parent_id = n.node_id)
            AND NOT EXISTS (SELECT 1 FROM refs r WHERE r.node_id = n.node_id)
            "#,
        )?;

        let nodes = stmt
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;

        Ok(nodes)
    }

    /// Find nodes by status
    pub fn find_nodes_by_status(&self, status: NodeStatus) -> Result<Vec<String>, DatabaseError> {
        let mut stmt = self
            .conn
            .prepare("SELECT node_id FROM nodes WHERE status = ?1")?;

        let nodes = stmt
            .query_map(params![status.as_str()], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;

        Ok(nodes)
    }

    /// Find nodes older than a timestamp
    pub fn find_nodes_older_than(&self, timestamp: i64) -> Result<Vec<String>, DatabaseError> {
        let mut stmt = self.conn.prepare(
            "SELECT node_id FROM nodes WHERE zone = 'exploration' AND created_at < ?1",
        )?;

        let nodes = stmt
            .query_map(params![timestamp], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;

        Ok(nodes)
    }

    /// Delete a node and all its edges
    pub fn delete_node(&mut self, node_id: &str) -> Result<(), DatabaseError> {
        // First, delete edges where this node is a parent (not covered by CASCADE)
        // The edges where this node is a child will be deleted by CASCADE
        self.conn
            .execute("DELETE FROM edges WHERE parent_id = ?1", params![node_id])?;
        
        // Now delete the node (CASCADE will handle child edges and files)
        self.conn
            .execute("DELETE FROM nodes WHERE node_id = ?1", params![node_id])?;
        Ok(())
    }

    /// Update the parents of a node (for squash operations)
    pub fn update_node_parents(
        &mut self,
        node_id: &str,
        new_parents: &[String],
    ) -> Result<(), DatabaseError> {
        let tx = self.conn.transaction()?;

        // Delete existing parent edges
        tx.execute(
            "DELETE FROM edges WHERE child_id = ?1",
            params![node_id],
        )?;

        // Insert new parent edges
        for parent_id in new_parents {
            tx.execute(
                "INSERT INTO edges (child_id, parent_id) VALUES (?1, ?2)",
                params![node_id, parent_id],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Update the intent of a node (for squash operations)
    pub fn update_node_intent(
        &mut self,
        node_id: &str,
        intent: &str,
    ) -> Result<(), DatabaseError> {
        self.conn.execute(
            "UPDATE nodes SET intent_prompt = ?1 WHERE node_id = ?2",
            params![intent, node_id],
        )?;
        Ok(())
    }

    /// Count total nodes
    pub fn count_nodes(&self) -> Result<usize, DatabaseError> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM nodes", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// Get all blob hashes in use
    pub fn get_all_blob_hashes(&self) -> Result<Vec<String>, DatabaseError> {
        let mut stmt = self.conn.prepare("SELECT DISTINCT blob_hash FROM files")?;

        let hashes = stmt
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;

        Ok(hashes)
    }

    /// Search files by path pattern
    pub fn search_files(&self, pattern: &str) -> Result<Vec<(String, FileEntry)>, DatabaseError> {
        let like_pattern = format!("%{}%", pattern);
        let mut stmt = self.conn.prepare(
            r#"
            SELECT node_id, path, blob_hash, mode
            FROM files
            WHERE path LIKE ?1
            ORDER BY node_id, path
            "#,
        )?;

        let results = stmt
            .query_map(params![like_pattern], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    FileEntry {
                        path: row.get(1)?,
                        blob_hash: row.get(2)?,
                        mode: row.get(3)?,
                    },
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::node::generate_node_id;

    #[test]
    fn test_database_init() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.count_nodes().unwrap(), 0);
    }

    #[test]
    fn test_insert_and_get_node() {
        let mut db = Database::open_in_memory().unwrap();

        let files = vec![FileEntry {
            path: "test.txt".to_string(),
            blob_hash: "abc123".to_string(),
            mode: 0o644,
        }];

        let tree_hash = "deadbeef".to_string();
        let node_id = generate_node_id(&[], &tree_hash, Utc::now());
        let node = Node::new(node_id.clone(), vec![], tree_hash, files).with_intent("test");

        db.insert_node(&node).unwrap();

        assert!(db.node_exists(&node_id).unwrap());

        let retrieved = db.get_node(&node_id).unwrap().unwrap();
        assert_eq!(retrieved.manifest.node_id, node_id);
        assert_eq!(retrieved.manifest.intent_prompt, Some("test".to_string()));
        assert_eq!(retrieved.files.len(), 1);
    }

    #[test]
    fn test_refs() {
        let mut db = Database::open_in_memory().unwrap();

        // Create a node first
        let node = Node::new("qv-test1".to_string(), vec![], "hash".to_string(), vec![]);
        db.insert_node(&node).unwrap();

        db.set_ref("HEAD", "qv-test1").unwrap();
        assert_eq!(db.get_ref("HEAD").unwrap(), Some("qv-test1".to_string()));

        db.delete_ref("HEAD").unwrap();
        assert_eq!(db.get_ref("HEAD").unwrap(), None);
    }
}

