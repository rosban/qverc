# qverc - Quantum Version Control

A next-generation Version Control System designed for AI agent workflows. Unlike Git which optimizes for linear, human-paced text accumulation, qverc treats the repository as a **Directed Acyclic Graph (DAG) of Verified System States**.

## Key Concepts

### State Nodes
The atomic unit of qverc is the **State Node** - a complete, self-contained system state that carries a semantic manifest:
- **Intent**: The purpose/prompt that generated this state
- **Agent Signature**: Which AI model created the code
- **Verification Status**: Draft → Valid → Verified → Spine
- **Metrics**: Build time, test results, etc.
- **Failure History**: Negative knowledge for future agents

### Zones
- **Exploration Zone**: Ephemeral branches where agents try multiple variants (Tier 1 verification)
- **Consolidation Zone (Spine)**: Production-ready, permanent nodes (Full verification)

### The Gatekeeper
Tiered verification system with configurable commands:
- **Tier 1**: Syntax check, linter (Draft → Valid)
- **Tier 2**: Unit tests (Valid → Verified)
- **Tier 3**: Full integration suite (Verified → Spine)

## Installation

```bash
# Build from source
cargo build --release

# Install globally
cargo install --path .
```

## Quick Start

```bash
# Initialize a new qverc repository
qverc init

# Configure verification in qverc.toml
cat > qverc.toml << 'EOF'
[gatekeeper]
tier1 = ["cargo check", "cargo clippy -- -D warnings"]
tier2 = ["cargo test --lib"]
tier3 = ["cargo test", "./scripts/integration.sh"]

[workspace]
ignore = [".qverc/", "target/", "node_modules/"]
EOF

# Start editing with an intent
qverc edit "Add user authentication"

# Make your changes...

# Sync changes to the graph (runs verification)
qverc sync --agent "gpt-4-turbo"

# View status
qverc status

# View history
qverc log

# Search for files
qverc query "auth"

# Checkout a previous state
qverc checkout qv-8d5e2a

# Garbage collect old exploration nodes
qverc prune --older-than 7d --execute
```

## Commands

| Command | Description |
|---------|-------------|
| `qverc init` | Initialize a new repository |
| `qverc edit [intent]` | Start editing with an optional intent |
| `qverc checkout <node>` | Restore workspace to a specific node state |
| `qverc sync` | Snapshot workspace, run verification, create node |
| `qverc status` | Show current workspace status and changes |
| `qverc log` | Display DAG history |
| `qverc promote` | Promote a node to the Spine (runs Tier 3 verification) |
| `qverc prune` | Garbage collect exploration nodes |
| `qverc query <pattern>` | Search files by path |
| `qverc merge run <nodes...>` | Prepare merge from multiple nodes |
| `qverc merge status` | Show current merge status |
| `qverc merge abort` | Abort current merge |
| `qverc squash <start> <end>` | Squash linear node sequence into one |

### checkout options
- `<node_id>`: Full or partial node ID (e.g., `qv-8d5e2a` or `8d5e2a`)
- `--force, -f`: Discard uncommitted changes and checkout anyway

### sync options
- `--agent <name>`: Set the agent signature
- `--skip-verify`: Skip gatekeeper verification

### log options
- `--limit <n>`: Number of nodes to show (default: 10)
- `--all`: Include exploration zone nodes
- `--json`: Output in JSON format (for tooling integration)

### promote options
- `<node_id>`: Node to promote (defaults to HEAD)
- `--skip-verify`: Skip Tier 3 verification
- `--force, -f`: Force promotion even for draft nodes

### prune options
- `--older-than <duration>`: Only prune nodes older than duration (e.g., "7d", "24h")
- `--failed-only`: Only prune draft/failed nodes
- `--orphaned`: Only prune orphaned nodes (no children)
- `--execute`: Actually delete (without this, dry-run only)

### status options
- `--json`: Output as JSON (for tooling integration)

### merge run options
- `<nodes...>`: Node IDs to merge (at least 2 required)
- `-i, --intent <text>`: Intent for the merged state

### squash options
- `<start_node>`: First node in the range to squash
- `<end_node>`: Last node in the range to squash
- `--include-spine`: Allow including spine node (end_node must be on spine)
- `-i, --intent <text>`: Custom intent for squashed node (default: combined intents)

## Configuration (qverc.toml)

```toml
[gatekeeper]
# Tier 1: Syntax/linter checks
tier1 = ["cargo check"]

# Tier 2: Unit tests
tier2 = ["cargo test --lib"]

# Tier 3: Full integration (for spine promotion)
tier3 = ["cargo test", "./scripts/full-integration.sh"]

[workspace]
# Patterns to ignore when scanning
ignore = [
    ".qverc/",
    "target/",
    "node_modules/",
    "*.log"
]

[plugins]
# Optional: path to vector store plugin for semantic search
# vector_store = "./plugins/lancedb-plugin.so"
```

## Repository Structure

```
your-project/
├── qverc.toml           # Configuration
├── .qvignore            # Ignore patterns (like .gitignore)
└── .qverc/
    ├── db.sqlite        # DAG database (nodes, edges, files)
    ├── objects/         # Content-addressed blob store
    │   ├── ab/
    │   │   └── cdef123...
    │   └── ...
    └── workspace.json   # Current workspace state
```

## VS Code / Cursor Extension

The `vscode-qverc` extension provides IDE integration:

### Features
- **File Decorations**: Tracked files show status badges (M=modified, A=added, D=deleted)
- **Status Bar**: Shows current HEAD node and workspace state
- **Graph Visualization**: Interactive DAG view from the command palette
- **Commands**: Init, sync, checkout, and more accessible via command palette

### Installation

```bash
# Build the extension
cd vscode-qverc
npm install
npx vsce package --allow-missing-repository

# Install in Cursor/VS Code
cursor --install-extension vscode-qverc-0.1.0.vsix
# or
code --install-extension vscode-qverc-0.1.0.vsix
```

### Extension Commands
- `qverc: Initialize Repository` - Run qverc init
- `qverc: Sync Changes` - Sync workspace to graph
- `qverc: Show Status` - Display current status
- `qverc: Show Graph` - Visualize the DAG
- `qverc: Checkout Node` - Switch to a specific node

## The "Vibe Loop" (AI Agent Workflow)

1. **Intent**: Developer runs `qverc edit "Make it faster"`
2. **Context**: Agent reads the codebase (or uses vector search if configured)
3. **Generation**: Agent writes files to the workspace
4. **Verification**: Agent runs `qverc sync`
   - System runs Gatekeeper checks
   - Measures metrics
   - Updates manifest
5. **Result**: If verified, node is added to the graph

## Merge Workflow (Agent-Friendly)

qverc uses an agent-friendly merge workflow designed for AI agents to synthesize solutions rather than pick winners:

```bash
# Start a merge from two nodes
qverc merge run qv-abc123 qv-def456 --intent "Combine auth and UI changes"

# This creates .qverc/merge/ with:
# - manifest.json: Conflict details and file status
# - intents.md: Human/agent-readable summary of intents
# - files/: All versions of conflicting files for context

# Check merge status
qverc merge status

# Agent resolves conflicts by writing final files to workspace
# Then sync to complete the merge
qverc sync

# Or abort to cancel
qverc merge abort
```

**Key differences from Git:**
- All versions of conflicting files are preserved in `.qverc/merge/files/`
- Agent has full context to synthesize a solution
- No auto-resolution or conflict markers in files
- Sync auto-detects resolved conflicts when files exist in workspace

## Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│                          CLI Layer                               │
│  init │ edit │ sync │ status │ log │ merge │ squash │ prune     │
└─────────────────────────────┬────────────────────────────────────┘
                              │
┌─────────────────────────────┴────────────────────────────────────┐
│                         Core Engine                              │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌─────────┐           │
│  │  Graph   │  │ Content  │  │Gatekeeper│  │Workspace│           │
│  │ Manager  │  │  Store   │  │          │  │ Manager │           │
│  └────┬─────┘  └────┬─────┘  └──────────┘  └─────────┘           │
└───────┼─────────────┼────────────────────────────────────────────┘
        │             │
┌───────┴─────────────┴────────────────────────────────────────────┐
│                        Storage Layer                             │
│  ┌──────────────────┐  ┌─────────────────────────────────────┐   │
│  │  SQLite (DAG)    │  │  .qverc/objects/ (blobs)            │   │
│  │  nodes, edges,   │  │  BLAKE3 content-addressed           │   │
│  │  files, refs     │  │  deduplication                      │   │
│  └──────────────────┘  └─────────────────────────────────────┘   │
└──────────────────────────────────────────────────────────────────┘
```

## Comparison with Git

| Aspect | Git | qverc |
|--------|-----|-------|
| Atomic Unit | Text diff | Complete system state |
| Optimization | Human-paced editing | High-velocity AI generation |
| Metadata | Author, message | Intent, agent, metrics, failures |
| Verification | Optional (hooks) | Built-in tiered Gatekeeper |
| Branches | Named refs | Zones (Exploration/Consolidation) |
| Storage | Pack files | BLAKE3 content-addressed |

## To Be Implemented

- [ ] **Intent-only merge** - Transfer ideas between nodes without transferring actual code. Could include a `.mergeignore` file to exclude specific files from merges, or a more sophisticated approach like extracting intent/patterns from code changes without copying implementation details. Useful when you want to apply a concept from one branch without its specific implementation.

- [ ] **Tier 1 & Tier 2 verification testing** - The gatekeeper tiers are implemented but haven't been thoroughly tested in real workflows. Need to verify that tier progression (Draft → Valid → Verified → Spine) works correctly with various project configurations and failure scenarios.

- [ ] **Enhanced query command** - Currently `qverc query` only searches file paths. Should be extended to search node intents, allowing queries like "find all nodes related to authentication" or "show nodes where intent mentions performance". Consider fuzzy matching and relevance ranking.

- [ ] **Comprehensive test suite** - Add unit tests and integration tests for core functionality: graph operations, CAS storage, merge conflicts, squash edge cases, workspace state management. Ensure robustness for edge cases like concurrent operations, large files, and corrupted state recovery.

- [x] **Worktrees (git-worktree equivalent)** - Support for multiple working directories connected to the same qverc repository. Allows parallel experimentation without constant switching.

## Qverc Worktrees

qverc supports multiple working directories connected to the same DAG, allowing you to work on different features in parallel.

```bash
# Create a new worktree check out to current HEAD
qverc worktree add ../feature-branch

# Create a worktree checked out to a specific node
qverc worktree add ../old-version qv-123456

# List all active worktrees
qverc worktree list

# Remove a worktree
qverc worktree remove ../feature-branch
```

**How it works:**
- The main repository holds the `.qverc/` directory with the database.
- Linked worktrees contain a `.qverc` file pointing to the main repository.
- Each worktree maintains its own `workspace.json` state.
- `qverc sync` creates new nodes in the shared graph, visible to all worktrees.

## License

MIT License - See [LICENSE](LICENSE) for details.

## Contributing

Contributions are welcome! This is an open-source project designed for the "Vibe Coding" era.

### Development

```bash
# Run tests
cargo test

# Run with debug output
RUST_LOG=debug cargo run -- <command>

# Check formatting
cargo fmt --check

# Run clippy
cargo clippy -- -D warnings
```

