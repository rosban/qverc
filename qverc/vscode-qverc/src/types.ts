/**
 * qverc VS Code Extension - Type Definitions
 */

/**
 * Node verification status
 */
export type NodeStatus = 'draft' | 'valid' | 'verified' | 'spine';

/**
 * Zone in the DAG
 */
export type Zone = 'exploration' | 'consolidation';

/**
 * Type of file change
 */
export type ChangeType = 'added' | 'modified' | 'deleted';

/**
 * A file change in the workspace
 */
export interface FileChange {
    path: string;
    type: ChangeType;
}

/**
 * Result of `qverc status` command
 */
export interface QvernStatus {
    /** Whether qverc is initialized in this workspace */
    initialized: boolean;
    /** Current HEAD node ID (e.g., "qv-abc123") */
    head: string | null;
    /** Current intent (from qverc edit) */
    intent: string | null;
    /** Node status */
    status: NodeStatus | null;
    /** Zone (exploration or consolidation) */
    zone: Zone | null;
    /** List of file changes */
    changes: FileChange[];
    /** Total number of nodes in the graph */
    nodeCount: number;
}

/**
 * A node in the DAG (from qverc log)
 */
export interface QvernNode {
    /** Node ID (e.g., "qv-abc123") */
    nodeId: string;
    /** Whether this is HEAD */
    isHead: boolean;
    /** Parent node IDs */
    parents: string[];
    /** Zone */
    zone: Zone;
    /** Status */
    status: NodeStatus;
    /** Intent/prompt */
    intent: string | null;
    /** Agent signature */
    agent: string | null;
    /** Tree hash */
    treeHash: string;
    /** Creation timestamp */
    createdAt: Date;
}

/**
 * Result of `qverc log` command
 */
export interface QvernLog {
    nodes: QvernNode[];
    totalCount: number;
}

/**
 * Extension configuration
 */
export interface QvernConfig {
    /** Path to qverc executable */
    executablePath: string;
    /** Auto-refresh status on file changes */
    autoRefresh: boolean;
    /** Refresh interval in milliseconds */
    refreshInterval: number;
}

/**
 * Result of a qverc CLI command
 */
export interface CommandResult {
    success: boolean;
    stdout: string;
    stderr: string;
    exitCode: number;
}

