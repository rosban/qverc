# Bootstrap Scripts

This folder contains migration and upgrade scripts for qverc. These scripts ensure backwards compatibility when:

1. **Schema changes**: Database schema updates (new fields, tables, indexes)
2. **Format changes**: Changes to blob storage, manifest format, or config structure
3. **Feature migrations**: Scripts to populate new fields from existing data
4. **Cross-version compatibility**: Allowing newer qverc to work with repos created by older versions

## Directory Structure

```
bootstrap/
├── README.md           # This file
├── migrations/         # Database schema migrations
│   └── NNNN_description.sql
├── scripts/            # General-purpose migration scripts
│   └── migrate_*.rs or migrate_*.sh
└── lib.rs              # Rust library for migration helpers (optional)
```

## Naming Convention

### SQL Migrations
- Format: `NNNN_short_description.sql`
- Example: `0001_add_metrics_table.sql`, `0002_add_vector_embeddings.sql`
- Migrations are run in numerical order

### Scripts
- Format: `migrate_<from_version>_to_<to_version>.sh` or `.rs`
- Example: `migrate_0.1.0_to_0.2.0.sh`

## Use Cases

### Example 1: Adding a new manifest field

When adding a new field like `embeddings_hash` to the manifest:

```sql
-- migrations/0001_add_embeddings_hash.sql
ALTER TABLE nodes ADD COLUMN embeddings_hash TEXT DEFAULT NULL;
```

### Example 2: Backfilling data

When a new feature requires computing data from existing nodes:

```bash
#!/bin/bash
# scripts/migrate_compute_embeddings.sh
# Iterates through all nodes and computes embeddings for files

for node_id in $(qverc query "." | grep "node" | awk '{print $2}'); do
    echo "Processing $node_id..."
    # Custom logic to compute and store embeddings
done
```

### Example 3: Schema version tracking

The database stores a `schema_version` in a metadata table:

```sql
-- Checked at startup
CREATE TABLE IF NOT EXISTS qverc_meta (
    key TEXT PRIMARY KEY,
    value TEXT
);

INSERT OR IGNORE INTO qverc_meta (key, value) VALUES ('schema_version', '1');
```

## Running Migrations

Migrations are typically run automatically when qverc detects an older schema version:

```bash
# Manual migration (future feature)
qverc migrate --from 0.1.0 --to 0.2.0

# Check current schema version
qverc info --schema
```

## Self-Hosting Consideration

Since qverc is used to version-control itself, these migration scripts must be:

1. **Idempotent**: Safe to run multiple times
2. **Backwards-compatible**: Don't break older nodes
3. **Tested**: Run against a copy of the database first
4. **Documented**: Clear comments explaining what each migration does

## Future: Vector Database Migrations

When the optional vector store plugin is added, migrations might include:

```python
# scripts/migrate_add_embeddings.py
# Backfill embeddings for all existing files

import qverc
from sentence_transformers import SentenceTransformer

model = SentenceTransformer('all-MiniLM-L6-v2')

for node in qverc.get_all_nodes():
    for file in node.files:
        if not file.has_embedding:
            content = qverc.cas.retrieve(file.blob_hash)
            embedding = model.encode(content.decode('utf-8', errors='ignore'))
            qverc.vector_store.upsert(file.blob_hash, embedding)
```

## Contributing

When adding a new feature that requires schema changes:

1. Create a migration script in `migrations/`
2. Update the schema version constant in `src/storage/database.rs`
3. Add migration logic to the startup sequence
4. Document the change in this README

