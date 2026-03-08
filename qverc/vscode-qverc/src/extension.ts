/**
 * qverc VS Code Extension
 * 
 * Main entry point for the extension.
 * Provides integration with qverc version control system.
 */

import * as vscode from 'vscode';
import { registerFileDecorationProvider, QvernFileDecorationProvider } from './fileDecorator';
import { registerStatusBar, QvernStatusBar } from './statusBar';
import { registerCommands, initOutputChannel } from './commands';
import { isInitialized } from './qvercCli';
import { ensureCursorRule } from './cursorRule';

let fileDecorator: QvernFileDecorationProvider | undefined;
let statusBar: QvernStatusBar | undefined;

/**
 * Extension activation
 */
export async function activate(context: vscode.ExtensionContext): Promise<void> {
    console.log('qverc extension activating...');

    // Get workspace root
    const workspaceFolders = vscode.workspace.workspaceFolders;
    if (!workspaceFolders || workspaceFolders.length === 0) {
        console.log('qverc: No workspace folder open');
        return;
    }

    const workspaceRoot = workspaceFolders[0].uri.fsPath;

    // Check if qverc is initialized
    const initialized = await isInitialized(workspaceRoot);
    vscode.commands.executeCommand('setContext', 'qverc.initialized', initialized);

    // Initialize output channel
    const outputChannel = initOutputChannel();
    context.subscriptions.push(outputChannel);

    // Register file decoration provider
    fileDecorator = registerFileDecorationProvider(context, workspaceRoot);

    // Register status bar
    statusBar = registerStatusBar(context, workspaceRoot);

    // Register commands
    registerCommands(context, fileDecorator, statusBar);

    if (initialized) {
        ensureCursorRule(workspaceRoot).catch(() => {});
    }

    // Watch for .qverc directory creation/deletion
    const qvercWatcher = vscode.workspace.createFileSystemWatcher(
        new vscode.RelativePattern(workspaceRoot, '.qverc/**')
    );

    qvercWatcher.onDidCreate(async () => {
        vscode.commands.executeCommand('setContext', 'qverc.initialized', true);
        await fileDecorator?.refresh();
        await statusBar?.refresh();
        ensureCursorRule(workspaceRoot).catch(() => {});
    });

    qvercWatcher.onDidDelete(async () => {
        const stillExists = await isInitialized(workspaceRoot);
        if (!stillExists) {
            vscode.commands.executeCommand('setContext', 'qverc.initialized', false);
            await statusBar?.refresh();
        }
    });

    context.subscriptions.push(qvercWatcher);

    console.log('qverc extension activated!');

    // Show welcome message if not initialized
    if (!initialized) {
        const action = await vscode.window.showInformationMessage(
            'qverc is not initialized in this workspace.',
            'Initialize',
            'Dismiss'
        );
        if (action === 'Initialize') {
            vscode.commands.executeCommand('qverc.init');
        }
    }
}

/**
 * Extension deactivation
 */
export function deactivate(): void {
    console.log('qverc extension deactivating...');
    
    fileDecorator?.dispose();
    statusBar?.dispose();
    
    console.log('qverc extension deactivated');
}
