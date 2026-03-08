/**
 * qverc Commands
 * 
 * Command handlers for the VS Code command palette.
 */

import * as vscode from 'vscode';
import * as qvercCli from './qvercCli';
import { QvernFileDecorationProvider } from './fileDecorator';
import { QvernStatusBar } from './statusBar';
import { QvernGraphView } from './graphView';
import { ensureCursorRule } from './cursorRule';

// Output channel for qverc messages
let outputChannel: vscode.OutputChannel;

/**
 * Initialize the output channel
 */
export function initOutputChannel(): vscode.OutputChannel {
    if (!outputChannel) {
        outputChannel = vscode.window.createOutputChannel('qverc');
    }
    return outputChannel;
}

/**
 * Get workspace root or show error
 */
function getWorkspaceRoot(): string | undefined {
    const workspaceFolders = vscode.workspace.workspaceFolders;
    if (!workspaceFolders || workspaceFolders.length === 0) {
        vscode.window.showErrorMessage('qverc: No workspace folder open');
        return undefined;
    }
    return workspaceFolders[0].uri.fsPath;
}

/**
 * Register all qverc commands
 */
export function registerCommands(
    context: vscode.ExtensionContext,
    fileDecorator: QvernFileDecorationProvider,
    statusBar: QvernStatusBar
): void {
    const output = initOutputChannel();

    // qverc.init - Initialize repository
    context.subscriptions.push(
        vscode.commands.registerCommand('qverc.init', async () => {
            const workspaceRoot = getWorkspaceRoot();
            if (!workspaceRoot) return;

            const result = await qvercCli.init(workspaceRoot);

            if (result.success) {
                vscode.window.showInformationMessage('qverc repository initialized!');
                vscode.commands.executeCommand('setContext', 'qverc.initialized', true);
                await refreshAll(fileDecorator, statusBar);
                ensureCursorRule(workspaceRoot).catch(() => {});
            } else {
                vscode.window.showErrorMessage(`qverc init failed: ${result.stderr}`);
            }

            output.appendLine(result.stdout);
            if (result.stderr) {
                output.appendLine(result.stderr);
            }
        })
    );

    // qverc.sync - Sync workspace
    context.subscriptions.push(
        vscode.commands.registerCommand('qverc.sync', async () => {
            const workspaceRoot = getWorkspaceRoot();
            if (!workspaceRoot) return;

            // Ask for agent name
            const agent = await vscode.window.showInputBox({
                prompt: 'Agent signature (optional)',
                placeHolder: 'e.g., gpt-4, claude-opus-4'
            });

            // Ask about verification
            const skipVerify = await vscode.window.showQuickPick(
                ['Run verification', 'Skip verification'],
                { placeHolder: 'Run gatekeeper verification?' }
            );

            const result = await vscode.window.withProgress(
                {
                    location: vscode.ProgressLocation.Notification,
                    title: 'qverc: Syncing...',
                    cancellable: false
                },
                async () => {
                    return await qvercCli.sync(
                        workspaceRoot,
                        agent || undefined,
                        skipVerify === 'Skip verification'
                    );
                }
            );

            if (result.success) {
                vscode.window.showInformationMessage('qverc sync complete!');
                await refreshAll(fileDecorator, statusBar);
            } else {
                vscode.window.showErrorMessage(`qverc sync failed: ${result.stderr}`);
            }

            output.appendLine(result.stdout);
            if (result.stderr) {
                output.appendLine(result.stderr);
            }
            output.show();
        })
    );

    // qverc.status - Show status
    context.subscriptions.push(
        vscode.commands.registerCommand('qverc.status', async () => {
            const workspaceRoot = getWorkspaceRoot();
            if (!workspaceRoot) return;

            const result = await qvercCli.executeCommand(workspaceRoot, ['status']);

            output.clear();
            output.appendLine('=== qverc status ===\n');
            output.appendLine(result.stdout);
            if (result.stderr) {
                output.appendLine(result.stderr);
            }
            output.show();
        })
    );

    // qverc.log - Show log
    context.subscriptions.push(
        vscode.commands.registerCommand('qverc.log', async () => {
            const workspaceRoot = getWorkspaceRoot();
            if (!workspaceRoot) return;

            const result = await qvercCli.executeCommand(workspaceRoot, ['log', '--all']);

            output.clear();
            output.appendLine('=== qverc log ===\n');
            output.appendLine(result.stdout);
            if (result.stderr) {
                output.appendLine(result.stderr);
            }
            output.show();
        })
    );

    // qverc.edit - Set intent
    context.subscriptions.push(
        vscode.commands.registerCommand('qverc.edit', async () => {
            const workspaceRoot = getWorkspaceRoot();
            if (!workspaceRoot) return;

            const intent = await vscode.window.showInputBox({
                prompt: 'Enter your intent for this edit',
                placeHolder: 'e.g., Add user authentication'
            });

            if (!intent) {
                return;
            }

            const result = await qvercCli.edit(workspaceRoot, intent);

            if (result.success) {
                vscode.window.showInformationMessage(`Intent set: ${intent}`);
                await refreshAll(fileDecorator, statusBar);
            } else {
                vscode.window.showErrorMessage(`qverc edit failed: ${result.stderr}`);
            }

            output.appendLine(result.stdout);
        })
    );

    // qverc.checkout - Checkout a node
    context.subscriptions.push(
        vscode.commands.registerCommand('qverc.checkout', async () => {
            const workspaceRoot = getWorkspaceRoot();
            if (!workspaceRoot) return;

            // Get list of nodes
            const log = await qvercCli.getLog(workspaceRoot, 20, true);

            if (log.nodes.length === 0) {
                vscode.window.showInformationMessage('No nodes in the graph');
                return;
            }

            // Create quick pick items
            const items = log.nodes.map(node => ({
                label: `${node.isHead ? '$(star) ' : ''}${node.nodeId}`,
                description: node.status,
                detail: node.intent || '(no intent)',
                nodeId: node.nodeId
            }));

            const selected = await vscode.window.showQuickPick(items, {
                placeHolder: 'Select a node to checkout'
            });

            if (!selected) {
                return;
            }

            // Check for uncommitted changes
            const status = await qvercCli.getStatus(workspaceRoot);
            let force = false;

            if (status.changes.length > 0) {
                const choice = await vscode.window.showWarningMessage(
                    `You have ${status.changes.length} uncommitted changes. Force checkout?`,
                    'Force Checkout',
                    'Cancel'
                );
                if (choice !== 'Force Checkout') {
                    return;
                }
                force = true;
            }

            const result = await vscode.window.withProgress(
                {
                    location: vscode.ProgressLocation.Notification,
                    title: `qverc: Checking out ${selected.nodeId}...`,
                    cancellable: false
                },
                async () => {
                    return await qvercCli.checkout(workspaceRoot, selected.nodeId, force);
                }
            );

            if (result.success) {
                vscode.window.showInformationMessage(`Checked out ${selected.nodeId}`);
                await refreshAll(fileDecorator, statusBar);
            } else {
                vscode.window.showErrorMessage(`Checkout failed: ${result.stderr}`);
            }

            output.appendLine(result.stdout);
        })
    );

    // qverc.refresh - Manual refresh
    context.subscriptions.push(
        vscode.commands.registerCommand('qverc.refresh', async () => {
            await refreshAll(fileDecorator, statusBar);
            vscode.window.showInformationMessage('qverc status refreshed');
        })
    );

    // qverc.showGraph - Show graph visualization
    context.subscriptions.push(
        vscode.commands.registerCommand('qverc.showGraph', async () => {
            const workspaceRoot = getWorkspaceRoot();
            if (!workspaceRoot) return;

            await QvernGraphView.createOrShow(context, workspaceRoot);
        })
    );

    // qverc.showGraphInBrowser - Open graph in system browser
    context.subscriptions.push(
        vscode.commands.registerCommand('qverc.showGraphInBrowser', async () => {
            const workspaceRoot = getWorkspaceRoot();
            if (!workspaceRoot) return;

            await QvernGraphView.openInBrowser(workspaceRoot);
        })
    );

    // qverc.showQuickPick - Show quick actions
    context.subscriptions.push(
        vscode.commands.registerCommand('qverc.showQuickPick', async () => {
            const workspaceRoot = getWorkspaceRoot();
            if (!workspaceRoot) return;

            const status = statusBar.getStatus();

            const items: vscode.QuickPickItem[] = [];

            if (!status?.initialized) {
                items.push({
                    label: '$(add) Initialize Repository',
                    description: 'qverc init'
                });
            } else {
                items.push(
                    {
                        label: '$(sync) Sync',
                        description: 'Commit changes to graph'
                    },
                    {
                        label: '$(edit) Set Intent',
                        description: 'Set editing intent'
                    },
                    {
                        label: '$(git-branch) Checkout',
                        description: 'Switch to another node'
                    },
                    {
                        label: '$(type-hierarchy) Show Graph',
                        description: 'Visualize DAG structure'
                    },
                    {
                        label: '$(globe) Show Graph in Browser',
                        description: 'Open graph in web browser'
                    },
                    {
                        label: '$(info) Show Status',
                        description: 'View current status'
                    },
                    {
                        label: '$(history) Show Log',
                        description: 'View DAG history'
                    },
                    {
                        label: '$(refresh) Refresh',
                        description: 'Refresh status'
                    }
                );
            }

            const selected = await vscode.window.showQuickPick(items, {
                placeHolder: 'qverc actions'
            });

            if (!selected) return;

            switch (selected.label) {
                case '$(add) Initialize Repository':
                    vscode.commands.executeCommand('qverc.init');
                    break;
                case '$(sync) Sync':
                    vscode.commands.executeCommand('qverc.sync');
                    break;
                case '$(edit) Set Intent':
                    vscode.commands.executeCommand('qverc.edit');
                    break;
                case '$(git-branch) Checkout':
                    vscode.commands.executeCommand('qverc.checkout');
                    break;
                case '$(type-hierarchy) Show Graph':
                    vscode.commands.executeCommand('qverc.showGraph');
                    break;
                case '$(globe) Show Graph in Browser':
                    vscode.commands.executeCommand('qverc.showGraphInBrowser');
                    break;
                case '$(info) Show Status':
                    vscode.commands.executeCommand('qverc.status');
                    break;
                case '$(history) Show Log':
                    vscode.commands.executeCommand('qverc.log');
                    break;
                case '$(refresh) Refresh':
                    vscode.commands.executeCommand('qverc.refresh');
                    break;
            }
        })
    );
}

/**
 * Refresh all providers
 */
async function refreshAll(
    fileDecorator: QvernFileDecorationProvider,
    statusBar: QvernStatusBar
): Promise<void> {
    await Promise.all([
        fileDecorator.refresh(),
        statusBar.refresh()
    ]);
}

