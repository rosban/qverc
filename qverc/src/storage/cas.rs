//! Content-Addressed Store (CAS)
//!
//! Stores file contents using BLAKE3 hashes for deduplication.

use blake3::Hasher;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CasError {
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Object not found: {0}")]
    ObjectNotFound(String),

    #[error("Invalid hash: {0}")]
    InvalidHash(String),
}

/// Content-Addressed Store
///
/// Files are stored at `.qverc/objects/{first 2 chars}/{remaining hash}`
pub struct ContentStore {
    objects_dir: PathBuf,
}

impl ContentStore {
    /// Create a new content store
    pub fn new(qvern_dir: impl AsRef<Path>) -> Self {
        Self {
            objects_dir: qvern_dir.as_ref().join("objects"),
        }
    }

    /// Initialize the content store directory
    pub fn init(&self) -> Result<(), CasError> {
        fs::create_dir_all(&self.objects_dir)?;
        Ok(())
    }

    /// Get the path for a given hash
    fn object_path(&self, hash: &str) -> PathBuf {
        if hash.len() < 3 {
            return self.objects_dir.join(hash);
        }
        let prefix = &hash[..2];
        let suffix = &hash[2..];
        self.objects_dir.join(prefix).join(suffix)
    }

    /// Hash file contents and return the BLAKE3 hash
    pub fn hash_bytes(data: &[u8]) -> String {
        let hash = blake3::hash(data);
        hash.to_hex().to_string()
    }

    /// Hash a file and return the BLAKE3 hash
    pub fn hash_file(path: impl AsRef<Path>) -> Result<String, CasError> {
        let mut file = File::open(path)?;
        let mut hasher = Hasher::new();
        let mut buffer = [0u8; 65536];

        loop {
            let bytes_read = file.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }

        Ok(hasher.finalize().to_hex().to_string())
    }

    /// Store bytes and return the hash
    pub fn store_bytes(&self, data: &[u8]) -> Result<String, CasError> {
        let hash = Self::hash_bytes(data);
        let path = self.object_path(&hash);

        // Already exists? Skip writing
        if path.exists() {
            return Ok(hash);
        }

        // Create parent directory
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Write to temp file, then rename (atomic)
        let temp_path = path.with_extension("tmp");
        {
            let mut file = File::create(&temp_path)?;
            file.write_all(data)?;
            file.sync_all()?;
        }
        fs::rename(&temp_path, &path)?;

        Ok(hash)
    }

    /// Store a file and return the hash
    pub fn store_file(&self, path: impl AsRef<Path>) -> Result<String, CasError> {
        let data = fs::read(path)?;
        self.store_bytes(&data)
    }

    /// Retrieve bytes by hash
    pub fn retrieve(&self, hash: &str) -> Result<Vec<u8>, CasError> {
        let path = self.object_path(hash);
        if !path.exists() {
            return Err(CasError::ObjectNotFound(hash.to_string()));
        }
        Ok(fs::read(path)?)
    }

    /// Check if an object exists
    pub fn exists(&self, hash: &str) -> bool {
        self.object_path(hash).exists()
    }

    /// Delete an object (for garbage collection)
    pub fn delete(&self, hash: &str) -> Result<(), CasError> {
        let path = self.object_path(hash);
        if path.exists() {
            fs::remove_file(&path)?;

            // Clean up empty parent directory
            if let Some(parent) = path.parent() {
                let _ = fs::remove_dir(parent); // Ignore error if not empty
            }
        }
        Ok(())
    }

    /// Get all stored object hashes
    pub fn list_objects(&self) -> Result<Vec<String>, CasError> {
        let mut objects = Vec::new();

        if !self.objects_dir.exists() {
            return Ok(objects);
        }

        for prefix_entry in fs::read_dir(&self.objects_dir)? {
            let prefix_entry = prefix_entry?;
            let prefix_path = prefix_entry.path();

            if !prefix_path.is_dir() {
                continue;
            }

            let prefix = prefix_entry
                .file_name()
                .to_string_lossy()
                .to_string();

            for obj_entry in fs::read_dir(&prefix_path)? {
                let obj_entry = obj_entry?;
                let suffix = obj_entry
                    .file_name()
                    .to_string_lossy()
                    .to_string();

                // Skip temp files
                if suffix.ends_with(".tmp") {
                    continue;
                }

                objects.push(format!("{}{}", prefix, suffix));
            }
        }

        Ok(objects)
    }

    /// Calculate storage usage in bytes
    pub fn storage_size(&self) -> Result<u64, CasError> {
        let mut total = 0u64;

        for hash in self.list_objects()? {
            let path = self.object_path(&hash);
            if let Ok(metadata) = fs::metadata(&path) {
                total += metadata.len();
            }
        }

        Ok(total)
    }

    /// Prune orphaned blobs that are not in the referenced set
    /// Returns the number of blobs deleted and bytes freed
    pub fn prune_orphaned(&self, referenced_hashes: &std::collections::HashSet<String>) -> Result<(usize, u64), CasError> {
        let mut deleted_count = 0usize;
        let mut bytes_freed = 0u64;

        for hash in self.list_objects()? {
            if !referenced_hashes.contains(&hash) {
                let path = self.object_path(&hash);
                if let Ok(metadata) = fs::metadata(&path) {
                    bytes_freed += metadata.len();
                }
                self.delete(&hash)?;
                deleted_count += 1;
            }
        }

        Ok((deleted_count, bytes_freed))
    }
}

/// Hash a directory tree and return a tree hash
///
/// The tree hash is computed from sorted (path, hash) pairs
pub fn hash_tree(entries: &[(String, String)]) -> String {
    let mut hasher = Hasher::new();

    // Sort by path for deterministic hashing
    let mut sorted: Vec<_> = entries.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));

    for (path, hash) in sorted {
        hasher.update(path.as_bytes());
        hasher.update(b"\0");
        hasher.update(hash.as_bytes());
        hasher.update(b"\n");
    }

    hasher.finalize().to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_hash_bytes() {
        let hash1 = ContentStore::hash_bytes(b"hello");
        let hash2 = ContentStore::hash_bytes(b"hello");
        let hash3 = ContentStore::hash_bytes(b"world");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
        assert_eq!(hash1.len(), 64); // BLAKE3 produces 256-bit hashes
    }

    #[test]
    fn test_store_and_retrieve() {
        let temp_dir = TempDir::new().unwrap();
        let store = ContentStore::new(temp_dir.path());
        store.init().unwrap();

        let data = b"test content";
        let hash = store.store_bytes(data).unwrap();

        assert!(store.exists(&hash));

        let retrieved = store.retrieve(&hash).unwrap();
        assert_eq!(retrieved, data);
    }

    #[test]
    fn test_deduplication() {
        let temp_dir = TempDir::new().unwrap();
        let store = ContentStore::new(temp_dir.path());
        store.init().unwrap();

        let data = b"duplicate content";
        let hash1 = store.store_bytes(data).unwrap();
        let hash2 = store.store_bytes(data).unwrap();

        assert_eq!(hash1, hash2);
        assert_eq!(store.list_objects().unwrap().len(), 1);
    }

    #[test]
    fn test_tree_hash() {
        let entries1 = vec![
            ("a.txt".to_string(), "hash1".to_string()),
            ("b.txt".to_string(), "hash2".to_string()),
        ];
        let entries2 = vec![
            ("b.txt".to_string(), "hash2".to_string()),
            ("a.txt".to_string(), "hash1".to_string()),
        ];

        // Order shouldn't matter (sorted internally)
        assert_eq!(hash_tree(&entries1), hash_tree(&entries2));
    }
}

