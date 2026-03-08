/**
 * qverc CLI Wrapper
 * 
 * Executes qverc commands and parses their output.
 */

import * as vscode from 'vscode';
import { spawn } from 'child_process';
import {
    QvernStatus,
    QvernLog,
    QvernNode,
    CommandResult,
    FileChange,
    ChangeType,
    NodeStatus,
    Zone
} from './types';

/**
 * Get the qverc executable path, checking common locations
 */
async function getExecutablePath(workspaceRoot: string): Promise<string> {
    const config = vscode.workspace.getConfiguration('qverc');
    const configuredPath = config.get<string>('executablePath', '');
    
    if (configuredPath && configuredPath !== 'qverc') {
        return configuredPath;
    }
    
    // Try common locations in the workspace
    const fs = await import('fs');
    const pathModule = await import('path');
    
    const candidates = [
        pathModule.join(workspaceRoot, 'target', 'release', 'qverc'),
        pathModule.join(workspaceRoot, 'target', 'debug', 'qverc'),
        'qverc' // Fall back to PATH
    ];
    
    for (const candidate of candidates) {
        if (candidate === 'qverc' || fs.existsSync(candidate)) {
            return candidate;
        }
    }
    
    return 'qverc';
}

/**
 * Execute a qverc command and return the result
 */
export async function executeCommand(
    workspaceRoot: string,
    args: string[]
): Promise<CommandResult> {
    const executablePath = await getExecutablePath(workspaceRoot);

    return new Promise((resolve) => {
        const proc = spawn(executablePath, args, {
            cwd: workspaceRoot,
            shell: true
        });

        let stdout = '';
        let stderr = '';

        proc.stdout.on('data', (data) => {
            stdout += data.toString();
        });

        proc.stderr.on('data', (data) => {
            stderr += data.toString();
        });

        proc.on('close', (code) => {
            resolve({
                success: code === 0,
                stdout,
                stderr,
                exitCode: code ?? -1
            });
        });

        proc.on('error', (err) => {
            resolve({
                success: false,
                stdout: '',
                stderr: err.message,
                exitCode: -1
            });
        });
    });
}

/**
 * Check if qverc is initialized in the workspace
 */
export async function isInitialized(workspaceRoot: string): Promise<boolean> {
    const fs = await import('fs');
    const path = await import('path');
    const qvercDir = path.join(workspaceRoot, '.qverc');
    return fs.existsSync(qvercDir);
}

/**
 * Get qverc status for the workspace
 */
export async function getStatus(workspaceRoot: string): Promise<QvernStatus> {
    const initialized = await isInitialized(workspaceRoot);
    
    if (!initialized) {
        return {
            initialized: false,
            head: null,
            intent: null,
            status: null,
            zone: null,
            changes: [],
            nodeCount: 0
        };
    }

    // Try JSON output first for reliable parsing
    const jsonResult = await executeCommand(workspaceRoot, ['status', '--json']);
    
    if (jsonResult.success && jsonResult.stdout.trim().startsWith('{')) {
        try {
            const json = JSON.parse(jsonResult.stdout);
            return {
                initialized: true,
                head: json.head || null,
                intent: null, // Not in JSON output yet
                status: json.status as NodeStatus || null,
                zone: json.zone as Zone || null,
                changes: (json.changes || []).map((c: { path: string; type: string }) => ({
                    path: c.path,
                    type: c.type as ChangeType
                })),
                nodeCount: json.graphNodes || 0
            };
        } catch {
            // Fall back to text parsing
        }
    }

    // Fall back to text parsing
    const result = await executeCommand(workspaceRoot, ['status']);
    
    if (!result.success) {
        return {
            initialized: true,
            head: null,
            intent: null,
            status: null,
            zone: null,
            changes: [],
            nodeCount: 0
        };
    }

    return parseStatusOutput(result.stdout);
}

/**
 * Parse the output of `qverc status`
 */
function parseStatusOutput(output: string): QvernStatus {
    const status: QvernStatus = {
        initialized: true,
        head: null,
        intent: null,
        status: null,
        zone: null,
        changes: [],
        nodeCount: 0
    };

    const lines = output.split('\n');
    
    for (const line of lines) {
        const trimmed = line.trim();
        
        // Parse HEAD
        if (trimmed.startsWith('HEAD:') || trimmed.startsWith('Current:')) {
            const match = trimmed.match(/(?:HEAD|Current):\s*(qv-[a-f0-9]+)/);
            if (match) {
                status.head = match[1];
            }
        }
        
        // Parse Intent
        if (trimmed.startsWith('Intent:')) {
            status.intent = trimmed.replace('Intent:', '').trim();
        }
        
        // Parse Status
        if (trimmed.startsWith('Status:')) {
            const statusMatch = trimmed.match(/Status:\s*(\w+)/);
            if (statusMatch) {
                status.status = statusMatch[1] as NodeStatus;
            }
        }
        
        // Parse Zone
        if (trimmed.startsWith('Zone:')) {
            const zoneMatch = trimmed.match(/Zone:\s*(\w+)/);
            if (zoneMatch) {
                status.zone = zoneMatch[1] as Zone;
            }
        }
        
        // Parse Graph count
        if (trimmed.startsWith('Graph:')) {
            const countMatch = trimmed.match(/Graph:\s*(\d+)/);
            if (countMatch) {
                status.nodeCount = parseInt(countMatch[1], 10);
            }
        }
        
        // Parse file changes
        // Format: "    + path/to/file" or "    ~ path/to/file" or "    - path/to/file"
        const changeMatch = trimmed.match(/^([+~-])\s+(.+)$/);
        if (changeMatch) {
            const type = changeMatch[1] === '+' ? 'added' 
                       : changeMatch[1] === '~' ? 'modified' 
                       : 'deleted';
            status.changes.push({
                path: changeMatch[2],
                type: type as ChangeType
            });
        }
    }

    return status;
}

/**
 * Get qverc log
 */
export async function getLog(
    workspaceRoot: string, 
    limit: number = 10, 
    all: boolean = true
): Promise<QvernLog> {
    const args = ['log', '--limit', limit.toString()];
    if (all) {
        args.push('--all');
    }
    
    const result = await executeCommand(workspaceRoot, args);
    
    if (!result.success) {
        return { nodes: [], totalCount: 0 };
    }

    return parseLogOutput(result.stdout);
}

/**
 * Parse the output of `qverc log`
 */
function parseLogOutput(output: string): QvernLog {
    const nodes: QvernNode[] = [];
    const lines = output.split('\n');
    
    let currentNode: Partial<QvernNode> | null = null;
    
    for (const line of lines) {
        const trimmed = line.trim();
        
        // New node starts with "node qv-..."
        const nodeMatch = trimmed.match(/^node\s+(qv-[a-f0-9]+)(?:\s+\(HEAD\))?/);
        if (nodeMatch) {
            if (currentNode && currentNode.nodeId) {
                nodes.push(currentNode as QvernNode);
            }
            currentNode = {
                nodeId: nodeMatch[1],
                isHead: trimmed.includes('(HEAD)'),
                parents: [],
                zone: 'exploration',
                status: 'draft',
                intent: null,
                agent: null,
                treeHash: '',
                createdAt: new Date()
            };
            continue;
        }
        
        if (!currentNode) continue;
        
        // Parse zone and status: "exploration | draft"
        const zoneStatusMatch = trimmed.match(/^(exploration|spine|consolidation)\s*\|\s*(\w+)/);
        if (zoneStatusMatch) {
            currentNode.zone = zoneStatusMatch[1] === 'spine' ? 'consolidation' : zoneStatusMatch[1] as Zone;
            currentNode.status = zoneStatusMatch[2] as NodeStatus;
            continue;
        }
        
        // Parse parent
        if (trimmed.startsWith('parent:')) {
            const parentMatch = trimmed.match(/parent:\s*(qv-[a-f0-9]+(?:,\s*qv-[a-f0-9]+)*)/);
            if (parentMatch) {
                currentNode.parents = parentMatch[1].split(',').map(p => p.trim());
            }
            continue;
        }
        
        // Parse timestamp (format: 2025-12-28 13:18:33)
        const timeMatch = trimmed.match(/^(\d{4}-\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2})/);
        if (timeMatch) {
            currentNode.createdAt = new Date(timeMatch[1].replace(' ', 'T'));
            continue;
        }
        
        // Parse agent
        if (trimmed.startsWith('agent:')) {
            currentNode.agent = trimmed.replace('agent:', '').trim();
            continue;
        }
        
        // Parse tree hash
        if (trimmed.startsWith('tree:')) {
            currentNode.treeHash = trimmed.replace('tree:', '').trim();
            continue;
        }
        
        // Intent is any other non-empty line after node info
        if (trimmed && !trimmed.startsWith('...') && currentNode.nodeId && !currentNode.intent) {
            // Skip if it looks like metadata
            if (!trimmed.includes(':') && !trimmed.startsWith('node')) {
                currentNode.intent = trimmed;
            }
        }
    }
    
    // Don't forget the last node
    if (currentNode && currentNode.nodeId) {
        nodes.push(currentNode as QvernNode);
    }
    
    // Parse total count from "Showing X of Y nodes" message
    let totalCount = nodes.length;
    const countMatch = output.match(/Showing\s+\d+\s+of\s+(\d+)\s+nodes/);
    if (countMatch) {
        totalCount = parseInt(countMatch[1], 10);
    }

    return { nodes, totalCount };
}

/**
 * Initialize qverc repository
 */
export async function init(workspaceRoot: string): Promise<CommandResult> {
    return executeCommand(workspaceRoot, ['init']);
}

/**
 * Sync workspace to graph
 */
export async function sync(
    workspaceRoot: string, 
    agent?: string,
    skipVerify: boolean = false
): Promise<CommandResult> {
    const args = ['sync'];
    if (agent) {
        args.push('--agent', agent);
    }
    if (skipVerify) {
        args.push('--skip-verify');
    }
    return executeCommand(workspaceRoot, args);
}

/**
 * Start editing with an intent
 */
export async function edit(workspaceRoot: string, intent: string): Promise<CommandResult> {
    return executeCommand(workspaceRoot, ['edit', intent]);
}

/**
 * Checkout a specific node
 */
export async function checkout(
    workspaceRoot: string, 
    nodeId: string,
    force: boolean = false
): Promise<CommandResult> {
    const args = ['checkout', nodeId];
    if (force) {
        args.push('--force');
    }
    return executeCommand(workspaceRoot, args);
}

/**
 * Promote a node to spine
 */
export async function promote(
    workspaceRoot: string,
    nodeId?: string,
    skipVerify: boolean = false,
    force: boolean = false
): Promise<CommandResult> {
    const args = ['promote'];
    if (nodeId) {
        args.push(nodeId);
    }
    if (skipVerify) {
        args.push('--skip-verify');
    }
    if (force) {
        args.push('--force');
    }
    return executeCommand(workspaceRoot, args);
}

