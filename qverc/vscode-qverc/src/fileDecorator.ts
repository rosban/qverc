/**
 * qverc File Decoration Provider
 * 
 * Provides visual decorations for files in the Explorer based on their qverc status.
 */

import * as vscode from 'vscode';
import * as path from 'path';
import { join } from 'path';
import { ChangeType } from './types';
import { getStatus } from './qvercCli';

/**
 * File decoration provider for qverc status
 */
export class QvernFileDecorationProvider implements vscode.FileDecorationProvider {
    private _onDidChangeFileDecorations = new vscode.EventEmitter<vscode.Uri | vscode.Uri[] | undefined>();
    readonly onDidChangeFileDecorations = this._onDidChangeFileDecorations.event;

    private workspaceRoot: string;
    private changes: Map<string, ChangeType> = new Map();
    private refreshTimer: NodeJS.Timeout | null = null;

    constructor(workspaceRoot: string) {
        this.workspaceRoot = workspaceRoot;
    }

    /**
     * Refresh file decorations by fetching current status
     * Uses differential updates to avoid blinking
     */
    async refresh(): Promise<void> {
        const status = await getStatus(this.workspaceRoot);
        
        // Build new changes map
        const newChanges = new Map<string, ChangeType>();
        for (const change of status.changes) {
            newChanges.set(change.path, change.type);
        }

        // Find URIs that actually changed (differential update)
        const changedUris: vscode.Uri[] = [];
        
        // Files that were removed from tracking or had status change
        for (const [filePath, oldType] of this.changes) {
            const newType = newChanges.get(filePath);
            if (oldType !== newType) {
                changedUris.push(vscode.Uri.file(join(this.workspaceRoot, filePath)));
            }
        }
        
        // Files that were newly added to tracking
        for (const [filePath] of newChanges) {
            if (!this.changes.has(filePath)) {
                changedUris.push(vscode.Uri.file(join(this.workspaceRoot, filePath)));
            }
        }

        // Update the changes map
        this.changes = newChanges;

        // Only fire if something actually changed, and only for specific URIs
        if (changedUris.length > 0) {
            this._onDidChangeFileDecorations.fire(changedUris);
        }
    }

    /**
     * Start auto-refresh timer
     */
    startAutoRefresh(): void {
        const config = vscode.workspace.getConfiguration('qverc');
        const autoRefresh = config.get<boolean>('autoRefresh', true);
        const interval = Math.max(config.get<number>('refreshInterval', 5000), 1000);

        if (autoRefresh && !this.refreshTimer) {
            this.refreshTimer = setInterval(() => {
                this.refresh();
            }, interval);
        }
    }

    /**
     * Stop auto-refresh timer
     */
    stopAutoRefresh(): void {
        if (this.refreshTimer) {
            clearInterval(this.refreshTimer);
            this.refreshTimer = null;
        }
    }

    /**
     * Provide decoration for a file
     */
    provideFileDecoration(uri: vscode.Uri): vscode.FileDecoration | undefined {
        // Only decorate files in the workspace
        if (!uri.fsPath.startsWith(this.workspaceRoot)) {
            return undefined;
        }

        // Get relative path
        const relativePath = path.relative(this.workspaceRoot, uri.fsPath);
        
        // Skip .qverc directory
        if (relativePath.startsWith('.qverc')) {
            return undefined;
        }

        const changeType = this.changes.get(relativePath);
        
        if (!changeType) {
            return undefined;
        }

        return this.getDecorationForType(changeType);
    }

    /**
     * Get decoration for a change type
     */
    private getDecorationForType(type: ChangeType): vscode.FileDecoration {
        switch (type) {
            case 'added':
                return {
                    badge: 'A',
                    tooltip: 'Added - New file not in current node',
                    color: new vscode.ThemeColor('qverc.addedForeground')
                };
            case 'modified':
                return {
                    badge: 'M',
                    tooltip: 'Modified - File changed from current node',
                    color: new vscode.ThemeColor('qverc.modifiedForeground')
                };
            case 'deleted':
                return {
                    badge: 'D',
                    tooltip: 'Deleted - File removed from current node',
                    color: new vscode.ThemeColor('qverc.deletedForeground')
                };
            default:
                return {};
        }
    }

    /**
     * Get current changes map (for external use)
     */
    getChanges(): Map<string, ChangeType> {
        return new Map(this.changes);
    }

    /**
     * Dispose of resources
     */
    dispose(): void {
        this.stopAutoRefresh();
        this._onDidChangeFileDecorations.dispose();
    }
}

/**
 * Create and register the file decoration provider
 */
export function registerFileDecorationProvider(
    context: vscode.ExtensionContext,
    workspaceRoot: string
): QvernFileDecorationProvider {
    const provider = new QvernFileDecorationProvider(workspaceRoot);
    
    // Register the provider
    const disposable = vscode.window.registerFileDecorationProvider(provider);
    context.subscriptions.push(disposable);
    context.subscriptions.push(provider);

    // Initial refresh
    provider.refresh();

    // Start auto-refresh
    provider.startAutoRefresh();

    // Watch for configuration changes
    context.subscriptions.push(
        vscode.workspace.onDidChangeConfiguration(e => {
            if (e.affectsConfiguration('qverc.autoRefresh') || 
                e.affectsConfiguration('qverc.refreshInterval')) {
                provider.stopAutoRefresh();
                provider.startAutoRefresh();
            }
        })
    );

    // Note: We intentionally don't use a file system watcher here.
    // The periodic auto-refresh timer handles updates efficiently with
    // differential updates that prevent blinking.

    return provider;
}

