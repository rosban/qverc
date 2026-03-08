/**
 * qverc Status Bar
 * 
 * Shows current node and status in the VS Code status bar.
 */

import * as vscode from 'vscode';
import { QvernStatus, NodeStatus } from './types';
import { getStatus } from './qvercCli';

/**
 * Status bar manager for qverc
 */
export class QvernStatusBar implements vscode.Disposable {
    private statusBarItem: vscode.StatusBarItem;
    private workspaceRoot: string;
    private refreshTimer: NodeJS.Timeout | null = null;
    private currentStatus: QvernStatus | null = null;

    constructor(workspaceRoot: string) {
        this.workspaceRoot = workspaceRoot;

        // Create status bar item (left side, high priority)
        this.statusBarItem = vscode.window.createStatusBarItem(
            vscode.StatusBarAlignment.Left,
            100
        );

        this.statusBarItem.command = 'qverc.showQuickPick';
        this.statusBarItem.show();
    }

    /**
     * Refresh the status bar
     */
    async refresh(): Promise<void> {
        try {
            this.currentStatus = await getStatus(this.workspaceRoot);
            this.updateDisplay();
        } catch (error) {
            this.statusBarItem.text = '$(error) qverc: error';
            this.statusBarItem.tooltip = 'Failed to get qverc status';
        }
    }

    /**
     * Update the status bar display
     */
    private updateDisplay(): void {
        if (!this.currentStatus) {
            this.statusBarItem.text = '$(question) qverc';
            this.statusBarItem.tooltip = 'qverc status unknown';
            return;
        }

        if (!this.currentStatus.initialized) {
            this.statusBarItem.text = '$(circle-slash) qverc: not initialized';
            this.statusBarItem.tooltip = 'Click to initialize qverc repository';
            this.statusBarItem.command = 'qverc.init';
            return;
        }

        const { head, status, zone, changes, intent } = this.currentStatus;

        // Build status text
        const icon = this.getStatusIcon(status);
        const nodeId = head ? head.substring(0, 9) : 'no HEAD';
        const changeCount = changes.length;
        
        let text = `${icon} ${nodeId}`;
        
        if (status) {
            text += ` (${status})`;
        }
        
        if (changeCount > 0) {
            text += ` +${changeCount}`;
        }

        this.statusBarItem.text = text;

        // Build tooltip
        let tooltip = `qverc: ${head || 'no HEAD'}\n`;
        tooltip += `Status: ${status || 'unknown'}\n`;
        tooltip += `Zone: ${zone || 'unknown'}\n`;
        
        if (intent) {
            tooltip += `Intent: ${intent}\n`;
        }
        
        if (changeCount > 0) {
            tooltip += `\n${changeCount} uncommitted change(s)`;
        } else {
            tooltip += '\nNo uncommitted changes';
        }
        
        tooltip += '\n\nClick for qverc actions';

        this.statusBarItem.tooltip = tooltip;
        this.statusBarItem.command = 'qverc.showQuickPick';

        // Update background color based on status
        this.statusBarItem.backgroundColor = this.getStatusBackground(status);
    }

    /**
     * Get icon for node status
     */
    private getStatusIcon(status: NodeStatus | null): string {
        switch (status) {
            case 'draft':
                return '$(circle-outline)';
            case 'valid':
                return '$(pass)';
            case 'verified':
                return '$(verified)';
            case 'spine':
                return '$(star-full)';
            default:
                return '$(git-branch)';
        }
    }

    /**
     * Get background color for status
     */
    private getStatusBackground(status: NodeStatus | null): vscode.ThemeColor | undefined {
        switch (status) {
            case 'draft':
                return new vscode.ThemeColor('statusBarItem.warningBackground');
            case 'spine':
                return undefined; // Use default
            default:
                return undefined;
        }
    }

    /**
     * Get current status
     */
    getStatus(): QvernStatus | null {
        return this.currentStatus;
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
     * Dispose of resources
     */
    dispose(): void {
        this.stopAutoRefresh();
        this.statusBarItem.dispose();
    }
}

/**
 * Create and register the status bar
 */
export function registerStatusBar(
    context: vscode.ExtensionContext,
    workspaceRoot: string
): QvernStatusBar {
    const statusBar = new QvernStatusBar(workspaceRoot);
    context.subscriptions.push(statusBar);

    // Initial refresh
    statusBar.refresh();

    // Start auto-refresh
    statusBar.startAutoRefresh();

    // Watch for configuration changes
    context.subscriptions.push(
        vscode.workspace.onDidChangeConfiguration(e => {
            if (e.affectsConfiguration('qverc.autoRefresh') || 
                e.affectsConfiguration('qverc.refreshInterval')) {
                statusBar.stopAutoRefresh();
                statusBar.startAutoRefresh();
            }
        })
    );

    return statusBar;
}

