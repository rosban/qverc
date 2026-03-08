/**
 * qverc Graph View
 * 
 * Webview panel for visualizing the DAG structure.
 */

import * as vscode from 'vscode';
import { getLog } from './qvercCli';
import { QvernNode } from './types';

export class QvernGraphView {
    public static currentPanel: QvernGraphView | undefined;
    private readonly _panel: vscode.WebviewPanel;
    private readonly _workspaceRoot: string;
    private _disposables: vscode.Disposable[] = [];

    private constructor(panel: vscode.WebviewPanel, workspaceRoot: string) {
        this._panel = panel;
        this._workspaceRoot = workspaceRoot;

        // Set initial content
        this._update();

        // Handle panel disposal
        this._panel.onDidDispose(() => this.dispose(), null, this._disposables);

        // Handle messages from webview
        this._panel.webview.onDidReceiveMessage(
            async (message) => {
                switch (message.command) {
                    case 'refresh':
                        await this._update();
                        break;
                    case 'checkout':
                        vscode.commands.executeCommand('qverc.checkoutNode', message.nodeId);
                        break;
                    case 'nodeClick':
                        this._showNodeDetails(message.nodeId);
                        break;
                }
            },
            null,
            this._disposables
        );
    }

    public static async createOrShow(context: vscode.ExtensionContext, workspaceRoot: string) {
        const column = vscode.ViewColumn.Beside;

        if (QvernGraphView.currentPanel) {
            QvernGraphView.currentPanel._panel.reveal(column);
            await QvernGraphView.currentPanel._update();
            return;
        }

        const panel = vscode.window.createWebviewPanel(
            'qvercGraph',
            'qverc Graph',
            column,
            {
                enableScripts: true,
                // Disabled retainContextWhenHidden - can cause ServiceWorker issues
                // retainContextWhenHidden: true,
                enableFindWidget: false,
                // Setting empty localResourceRoots prevents ServiceWorker issues
                localResourceRoots: []
            }
        );

        QvernGraphView.currentPanel = new QvernGraphView(panel, workspaceRoot);
    }

    private async _update() {
        const log = await getLog(this._workspaceRoot, 100, true);
        this._panel.webview.html = this._getHtmlContent(this._panel.webview, log.nodes, log.totalCount);
    }

    private _showNodeDetails(nodeId: string) {
        vscode.window.showInformationMessage(`Node: ${nodeId}`);
    }

    private _getHtmlContent(webview: vscode.Webview, nodes: QvernNode[], totalCount: number): string {
        // Build node map for efficient lookup
        const nodeMap = new Map<string, QvernNode>();
        nodes.forEach(n => nodeMap.set(n.nodeId, n));

        // Calculate layout positions using topological sort
        const positions = this._calculateLayout(nodes);

        // Generate a nonce for inline scripts (required for CSP)
        const nonce = this._getNonce();

        // Use webview's cspSource for proper security
        const cspSource = webview.cspSource;

        return `<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${cspSource} 'unsafe-inline'; script-src 'nonce-${nonce}'; img-src ${cspSource} data:;">
    <title>qverc Graph</title>
    <style>
        :root {
            --bg-color: #1e1e2e;
            --surface-color: #313244;
            --text-color: #cdd6f4;
            --text-muted: #6c7086;
            --accent-blue: #89b4fa;
            --accent-green: #a6e3a1;
            --accent-yellow: #f9e2af;
            --accent-red: #f38ba8;
            --accent-cyan: #94e2d5;
            --accent-mauve: #cba6f7;
            --edge-color: #45475a;
        }
        
        * { box-sizing: border-box; margin: 0; padding: 0; }
        
        body {
            background: var(--bg-color);
            color: var(--text-color);
            font-family: 'JetBrains Mono', 'Fira Code', 'SF Mono', monospace;
            font-size: 12px;
            overflow: hidden;
            height: 100vh;
        }
        
        .header {
            display: flex;
            align-items: center;
            justify-content: space-between;
            padding: 12px 16px;
            background: var(--surface-color);
            border-bottom: 1px solid var(--edge-color);
        }
        
        .header h1 {
            font-size: 14px;
            font-weight: 600;
            color: var(--accent-mauve);
        }
        
        .header .stats {
            color: var(--text-muted);
            font-size: 11px;
        }
        
        .refresh-btn {
            background: var(--accent-blue);
            color: var(--bg-color);
            border: none;
            padding: 6px 12px;
            border-radius: 4px;
            cursor: pointer;
            font-size: 11px;
            font-weight: 600;
        }
        
        .refresh-btn:hover {
            opacity: 0.9;
        }
        
        .graph-container {
            width: 100%;
            height: calc(100vh - 50px);
            overflow: auto;
            padding: 20px;
        }
        
        .graph-svg {
            display: block;
            min-width: 100%;
        }
        
        .node-group {
            cursor: pointer;
        }
        
        .node-group:hover .node-rect {
            filter: brightness(1.2);
        }
        
        .node-rect {
            rx: 6;
            ry: 6;
            stroke-width: 2;
            transition: filter 0.15s ease;
        }
        
        .node-rect.spine { fill: #1e3a5f; stroke: var(--accent-cyan); }
        .node-rect.verified { fill: #1e3f2e; stroke: var(--accent-green); }
        .node-rect.valid { fill: #3f3a1e; stroke: var(--accent-yellow); }
        .node-rect.draft { fill: #3f1e2e; stroke: var(--accent-red); }
        
        .node-id {
            font-size: 11px;
            font-weight: 600;
            fill: var(--text-color);
        }
        
        .node-status {
            font-size: 9px;
            fill: var(--text-muted);
        }
        
        .node-badge {
            font-size: 8px;
            font-weight: bold;
        }
        
        .head-badge { fill: var(--accent-yellow); }
        .spine-badge { fill: var(--accent-cyan); }
        
        .edge {
            stroke: var(--edge-color);
            stroke-width: 2;
            fill: none;
            marker-end: url(#arrowhead);
        }
        
        .edge.to-spine {
            stroke: var(--accent-cyan);
            stroke-opacity: 0.6;
        }
        
        .tooltip {
            position: fixed;
            background: var(--surface-color);
            border: 1px solid var(--edge-color);
            border-radius: 6px;
            padding: 10px 12px;
            max-width: 300px;
            box-shadow: 0 4px 12px rgba(0,0,0,0.4);
            pointer-events: none;
            opacity: 0;
            transition: opacity 0.15s ease;
            z-index: 1000;
        }
        
        .tooltip.visible { opacity: 1; }
        
        .tooltip-title {
            font-weight: 600;
            color: var(--accent-blue);
            margin-bottom: 6px;
        }
        
        .tooltip-row {
            display: flex;
            margin: 3px 0;
        }
        
        .tooltip-label {
            color: var(--text-muted);
            width: 60px;
            flex-shrink: 0;
        }
        
        .tooltip-value {
            color: var(--text-color);
            word-break: break-word;
        }
        
        .legend {
            position: fixed;
            bottom: 16px;
            right: 16px;
            background: var(--surface-color);
            border: 1px solid var(--edge-color);
            border-radius: 6px;
            padding: 10px 14px;
            font-size: 10px;
        }
        
        .legend-item {
            display: flex;
            align-items: center;
            margin: 4px 0;
        }
        
        .legend-dot {
            width: 10px;
            height: 10px;
            border-radius: 2px;
            margin-right: 8px;
        }
        
        .legend-dot.spine { background: var(--accent-cyan); }
        .legend-dot.verified { background: var(--accent-green); }
        .legend-dot.valid { background: var(--accent-yellow); }
        .legend-dot.draft { background: var(--accent-red); }
    </style>
</head>
<body>
    <div class="header">
        <h1>⊛ qverc graph</h1>
        <span class="stats">${nodes.length} of ${totalCount} nodes</span>
        <button class="refresh-btn" onclick="refresh()">↻ Refresh</button>
    </div>
    
    <div class="graph-container" id="graphContainer">
        ${this._renderGraph(nodes, positions)}
    </div>
    
    <div class="tooltip" id="tooltip"></div>
    
    <div class="legend">
        <div class="legend-item"><div class="legend-dot spine"></div> spine</div>
        <div class="legend-item"><div class="legend-dot verified"></div> verified</div>
        <div class="legend-item"><div class="legend-dot valid"></div> valid</div>
        <div class="legend-item"><div class="legend-dot draft"></div> draft</div>
    </div>
    
    <script nonce="${nonce}">
        const vscode = acquireVsCodeApi();
        const nodesData = ${JSON.stringify(nodes)};
        
        function refresh() {
            vscode.postMessage({ command: 'refresh' });
        }
        
        function onNodeClick(nodeId) {
            vscode.postMessage({ command: 'nodeClick', nodeId });
        }
        
        function onNodeDblClick(nodeId) {
            vscode.postMessage({ command: 'checkout', nodeId });
        }
        
        // Tooltip handling
        const tooltip = document.getElementById('tooltip');
        
        document.querySelectorAll('.node-group').forEach(node => {
            node.addEventListener('mouseenter', (e) => {
                const nodeId = node.dataset.nodeId;
                const data = nodesData.find(n => n.nodeId === nodeId);
                if (!data) return;
                
                tooltip.innerHTML = \`
                    <div class="tooltip-title">\${data.nodeId}</div>
                    <div class="tooltip-row">
                        <span class="tooltip-label">Status:</span>
                        <span class="tooltip-value">\${data.status}</span>
                    </div>
                    <div class="tooltip-row">
                        <span class="tooltip-label">Zone:</span>
                        <span class="tooltip-value">\${data.zone}</span>
                    </div>
                    \${data.intent ? \`<div class="tooltip-row">
                        <span class="tooltip-label">Intent:</span>
                        <span class="tooltip-value">\${data.intent}</span>
                    </div>\` : ''}
                    \${data.agent ? \`<div class="tooltip-row">
                        <span class="tooltip-label">Agent:</span>
                        <span class="tooltip-value">\${data.agent}</span>
                    </div>\` : ''}
                    <div class="tooltip-row">
                        <span class="tooltip-label">Time:</span>
                        <span class="tooltip-value">\${new Date(data.createdAt).toLocaleString()}</span>
                    </div>
                \`;
                
                tooltip.classList.add('visible');
            });
            
            node.addEventListener('mousemove', (e) => {
                tooltip.style.left = (e.clientX + 15) + 'px';
                tooltip.style.top = (e.clientY + 15) + 'px';
            });
            
            node.addEventListener('mouseleave', () => {
                tooltip.classList.remove('visible');
            });
        });
    </script>
</body>
</html>`;
    }

    private _calculateLayout(nodes: QvernNode[]): Map<string, { x: number; y: number; level: number }> {
        const positions = new Map<string, { x: number; y: number; level: number }>();

        if (nodes.length === 0) return positions;

        // Build parent-child relationships
        const children = new Map<string, string[]>();
        const parentsMap = new Map<string, string[]>();
        const nodeIds = new Set(nodes.map(n => n.nodeId));

        nodes.forEach(n => {
            // Only include parents that are in our node set
            const validParents = n.parents.filter(p => nodeIds.has(p));
            parentsMap.set(n.nodeId, validParents);
            validParents.forEach(p => {
                if (!children.has(p)) children.set(p, []);
                children.get(p)!.push(n.nodeId);
            });
        });

        // Calculate levels using topological sort with max parent level
        // A node's level = max(parent levels) + 1
        // This ensures merge nodes appear below ALL their parents
        const levels = new Map<string, number>();

        const calculateLevel = (nodeId: string, visited: Set<string>): number => {
            if (levels.has(nodeId)) return levels.get(nodeId)!;
            if (visited.has(nodeId)) return 0; // Cycle protection

            visited.add(nodeId);
            const nodeParents = parentsMap.get(nodeId) || [];

            if (nodeParents.length === 0) {
                levels.set(nodeId, 0);
                return 0;
            }

            // Level is max of all parent levels + 1
            let maxParentLevel = -1;
            for (const parent of nodeParents) {
                const parentLevel = calculateLevel(parent, visited);
                maxParentLevel = Math.max(maxParentLevel, parentLevel);
            }

            const level = maxParentLevel + 1;
            levels.set(nodeId, level);
            return level;
        };

        // Calculate level for all nodes
        nodes.forEach(n => calculateLevel(n.nodeId, new Set()));

        // Group by level
        const levelGroups = new Map<number, string[]>();
        levels.forEach((level, id) => {
            if (!levelGroups.has(level)) levelGroups.set(level, []);
            levelGroups.get(level)!.push(id);
        });

        // Sort nodes within each level for consistent ordering
        // Use creation time if available, otherwise alphabetically
        const nodeMap = new Map(nodes.map(n => [n.nodeId, n]));
        levelGroups.forEach((ids, level) => {
            ids.sort((a, b) => {
                const nodeA = nodeMap.get(a);
                const nodeB = nodeMap.get(b);
                if (nodeA?.createdAt && nodeB?.createdAt) {
                    return new Date(nodeA.createdAt).getTime() - new Date(nodeB.createdAt).getTime();
                }
                return a.localeCompare(b);
            });
        });

        // Calculate positions
        const nodeWidth = 140;
        const nodeHeight = 50;
        const levelSpacing = 100;
        const nodeSpacing = 30;

        levelGroups.forEach((ids, level) => {
            const totalWidth = ids.length * nodeWidth + (ids.length - 1) * nodeSpacing;
            let startX = 50;

            ids.forEach((id, idx) => {
                positions.set(id, {
                    x: startX + idx * (nodeWidth + nodeSpacing),
                    y: 30 + level * (nodeHeight + levelSpacing),
                    level
                });
            });
        });

        return positions;
    }

    private _renderGraph(nodes: QvernNode[], positions: Map<string, { x: number; y: number; level: number }>): string {
        if (nodes.length === 0) {
            return '<p style="color: var(--text-muted); text-align: center; margin-top: 40px;">No nodes in graph</p>';
        }

        const nodeWidth = 140;
        const nodeHeight = 50;

        // Calculate SVG dimensions
        let maxX = 0, maxY = 0;
        positions.forEach(pos => {
            maxX = Math.max(maxX, pos.x + nodeWidth);
            maxY = Math.max(maxY, pos.y + nodeHeight);
        });

        const svgWidth = maxX + 50;
        const svgHeight = maxY + 50;

        // Build edges SVG
        const edges: string[] = [];
        nodes.forEach(node => {
            const pos = positions.get(node.nodeId);
            if (!pos) return;

            node.parents.forEach(parentId => {
                const parentPos = positions.get(parentId);
                if (!parentPos) return;

                const startX = parentPos.x + nodeWidth / 2;
                const startY = parentPos.y + nodeHeight;
                const endX = pos.x + nodeWidth / 2;
                const endY = pos.y;

                // Bezier curve for smooth edges
                const midY = (startY + endY) / 2;
                const isSpineEdge = node.status === 'spine';

                edges.push(`
                    <path class="edge ${isSpineEdge ? 'to-spine' : ''}" 
                          d="M ${startX} ${startY} C ${startX} ${midY}, ${endX} ${midY}, ${endX} ${endY}" />
                `);
            });
        });

        // Build nodes SVG
        const nodeSvg: string[] = [];
        nodes.forEach(node => {
            const pos = positions.get(node.nodeId);
            if (!pos) return;

            const shortId = node.nodeId.substring(0, 9);
            const badges: string[] = [];

            if (node.isHead) {
                badges.push(`<text class="node-badge head-badge" x="${pos.x + nodeWidth - 8}" y="${pos.y + 12}" text-anchor="end">HEAD</text>`);
            }
            if (node.status === 'spine') {
                badges.push(`<text class="node-badge spine-badge" x="${pos.x + nodeWidth - 8}" y="${pos.y + nodeHeight - 8}" text-anchor="end">SPINE</text>`);
            }

            nodeSvg.push(`
                <g class="node-group" data-node-id="${node.nodeId}" 
                   onclick="onNodeClick('${node.nodeId}')" 
                   ondblclick="onNodeDblClick('${node.nodeId}')">
                    <rect class="node-rect ${node.status}" 
                          x="${pos.x}" y="${pos.y}" 
                          width="${nodeWidth}" height="${nodeHeight}" />
                    <text class="node-id" x="${pos.x + 10}" y="${pos.y + 22}">${shortId}</text>
                    <text class="node-status" x="${pos.x + 10}" y="${pos.y + 38}">${node.zone} · ${node.status}</text>
                    ${badges.join('')}
                </g>
            `);
        });

        return `
            <svg class="graph-svg" width="${svgWidth}" height="${svgHeight}" viewBox="0 0 ${svgWidth} ${svgHeight}">
                <defs>
                    <marker id="arrowhead" markerWidth="10" markerHeight="7" 
                            refX="9" refY="3.5" orient="auto">
                        <polygon points="0 0, 10 3.5, 0 7" fill="#45475a" />
                    </marker>
                </defs>
                <g class="edges">${edges.join('')}</g>
                <g class="nodes">${nodeSvg.join('')}</g>
            </svg>
        `;
    }

    /**
     * Generate a random nonce for CSP
     */
    private _getNonce(): string {
        let text = '';
        const possible = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';
        for (let i = 0; i < 32; i++) {
            text += possible.charAt(Math.floor(Math.random() * possible.length));
        }
        return text;
    }

    public dispose() {
        QvernGraphView.currentPanel = undefined;
        this._panel.dispose();
        while (this._disposables.length) {
            const disposable = this._disposables.pop();
            if (disposable) disposable.dispose();
        }
    }

    /**
     * Open graph in system default browser (workaround for webview issues)
     */
    public static async openInBrowser(workspaceRoot: string): Promise<void> {
        const fs = await import('fs');
        const path = await import('path');
        const os = await import('os');

        try {
            // Get log data
            const log = await getLog(workspaceRoot, 100, true);

            if (log.nodes.length === 0) {
                vscode.window.showInformationMessage('No nodes in graph');
                return;
            }

            // Generate standalone HTML (no VS Code webview dependencies)
            const html = QvernGraphView._generateStandaloneHtml(log.nodes, log.totalCount);

            // Write to temp file
            const tempDir = os.tmpdir();
            const tempFile = path.join(tempDir, `qverc-graph-${Date.now()}.html`);
            fs.writeFileSync(tempFile, html, 'utf8');

            // Open in default browser
            const uri = vscode.Uri.file(tempFile);
            await vscode.env.openExternal(uri);

            vscode.window.showInformationMessage(`Graph opened in browser: ${tempFile}`);
        } catch (error) {
            vscode.window.showErrorMessage(`Failed to open graph in browser: ${error}`);
        }
    }

    /**
     * Generate standalone HTML for browser viewing (no CSP/nonce needed)
     */
    private static _generateStandaloneHtml(nodes: QvernNode[], totalCount: number): string {
        // Calculate layout
        const positions = QvernGraphView._calculateLayoutStatic(nodes);

        // Generate SVG
        const svgContent = QvernGraphView._renderGraphStatic(nodes, positions);

        return `<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>qverc Graph</title>
    <style>
        :root {
            --bg-color: #1e1e2e;
            --surface-color: #313244;
            --text-color: #cdd6f4;
            --text-muted: #6c7086;
            --accent-blue: #89b4fa;
            --accent-green: #a6e3a1;
            --accent-yellow: #f9e2af;
            --accent-red: #f38ba8;
            --accent-cyan: #94e2d5;
            --accent-mauve: #cba6f7;
            --edge-color: #45475a;
        }
        
        * { box-sizing: border-box; margin: 0; padding: 0; }
        
        body {
            background: var(--bg-color);
            color: var(--text-color);
            font-family: 'JetBrains Mono', 'Fira Code', 'SF Mono', monospace;
            font-size: 12px;
            overflow: auto;
            min-height: 100vh;
            padding: 20px;
        }
        
        .header {
            display: flex;
            align-items: center;
            justify-content: space-between;
            padding: 12px 16px;
            background: var(--surface-color);
            border-radius: 8px;
            margin-bottom: 20px;
        }
        
        .header h1 {
            font-size: 16px;
            font-weight: 600;
            color: var(--accent-mauve);
        }
        
        .header .stats {
            color: var(--text-muted);
            font-size: 12px;
        }
        
        .graph-container {
            background: var(--surface-color);
            border-radius: 8px;
            padding: 20px;
            overflow: auto;
        }
        
        .graph-svg {
            display: block;
            min-width: 100%;
        }
        
        .node-group {
            cursor: pointer;
        }
        
        .node-group:hover .node-rect {
            filter: brightness(1.2);
        }
        
        .node-rect {
            rx: 6;
            ry: 6;
            stroke-width: 2;
            transition: filter 0.15s ease;
        }
        
        .node-rect.spine { fill: #1e3a5f; stroke: var(--accent-cyan); }
        .node-rect.verified { fill: #1e3f2e; stroke: var(--accent-green); }
        .node-rect.valid { fill: #3f3a1e; stroke: var(--accent-yellow); }
        .node-rect.draft { fill: #3f1e2e; stroke: var(--accent-red); }
        
        .node-id {
            font-size: 11px;
            font-weight: 600;
            fill: var(--text-color);
        }
        
        .node-status {
            font-size: 9px;
            fill: var(--text-muted);
        }
        
        .node-badge {
            font-size: 8px;
            font-weight: bold;
        }
        
        .head-badge { fill: var(--accent-yellow); }
        .spine-badge { fill: var(--accent-cyan); }
        
        .edge {
            stroke: var(--edge-color);
            stroke-width: 2;
            fill: none;
            marker-end: url(#arrowhead);
        }
        
        .edge.to-spine {
            stroke: var(--accent-cyan);
            stroke-opacity: 0.6;
        }
        
        .legend {
            position: fixed;
            bottom: 16px;
            right: 16px;
            background: var(--surface-color);
            border: 1px solid var(--edge-color);
            border-radius: 6px;
            padding: 10px 14px;
            font-size: 10px;
        }
        
        .legend-item {
            display: flex;
            align-items: center;
            margin: 4px 0;
        }
        
        .legend-dot {
            width: 10px;
            height: 10px;
            border-radius: 2px;
            margin-right: 8px;
        }
        
        .legend-dot.spine { background: var(--accent-cyan); }
        .legend-dot.verified { background: var(--accent-green); }
        .legend-dot.valid { background: var(--accent-yellow); }
        .legend-dot.draft { background: var(--accent-red); }

        .tooltip {
            position: fixed;
            background: var(--surface-color);
            border: 1px solid var(--edge-color);
            border-radius: 6px;
            padding: 10px 12px;
            max-width: 300px;
            box-shadow: 0 4px 12px rgba(0,0,0,0.4);
            pointer-events: none;
            opacity: 0;
            transition: opacity 0.15s ease;
            z-index: 1000;
        }
        
        .tooltip.visible { opacity: 1; }
        
        .tooltip-title {
            font-weight: 600;
            color: var(--accent-blue);
            margin-bottom: 6px;
        }
        
        .tooltip-row {
            display: flex;
            margin: 3px 0;
        }
        
        .tooltip-label {
            color: var(--text-muted);
            width: 60px;
            flex-shrink: 0;
        }
        
        .tooltip-value {
            color: var(--text-color);
            word-break: break-word;
        }
    </style>
</head>
<body>
    <div class="header">
        <h1>⊛ qverc graph</h1>
        <span class="stats">${nodes.length} of ${totalCount} nodes</span>
    </div>
    
    <div class="graph-container">
        ${svgContent}
    </div>
    
    <div class="tooltip" id="tooltip"></div>
    
    <div class="legend">
        <div class="legend-item"><div class="legend-dot spine"></div> spine</div>
        <div class="legend-item"><div class="legend-dot verified"></div> verified</div>
        <div class="legend-item"><div class="legend-dot valid"></div> valid</div>
        <div class="legend-item"><div class="legend-dot draft"></div> draft</div>
    </div>
    
    <script>
        const nodesData = ${JSON.stringify(nodes)};
        const tooltip = document.getElementById('tooltip');
        
        document.querySelectorAll('.node-group').forEach(node => {
            node.addEventListener('mouseenter', (e) => {
                const nodeId = node.dataset.nodeId;
                const data = nodesData.find(n => n.nodeId === nodeId);
                if (!data) return;
                
                tooltip.innerHTML = \`
                    <div class="tooltip-title">\${data.nodeId}</div>
                    <div class="tooltip-row">
                        <span class="tooltip-label">Status:</span>
                        <span class="tooltip-value">\${data.status}</span>
                    </div>
                    <div class="tooltip-row">
                        <span class="tooltip-label">Zone:</span>
                        <span class="tooltip-value">\${data.zone}</span>
                    </div>
                    \${data.intent ? \`<div class="tooltip-row">
                        <span class="tooltip-label">Intent:</span>
                        <span class="tooltip-value">\${data.intent}</span>
                    </div>\` : ''}
                    \${data.agent ? \`<div class="tooltip-row">
                        <span class="tooltip-label">Agent:</span>
                        <span class="tooltip-value">\${data.agent}</span>
                    </div>\` : ''}
                    <div class="tooltip-row">
                        <span class="tooltip-label">Time:</span>
                        <span class="tooltip-value">\${new Date(data.createdAt).toLocaleString()}</span>
                    </div>
                \`;
                
                tooltip.classList.add('visible');
            });
            
            node.addEventListener('mousemove', (e) => {
                tooltip.style.left = (e.clientX + 15) + 'px';
                tooltip.style.top = (e.clientY + 15) + 'px';
            });
            
            node.addEventListener('mouseleave', () => {
                tooltip.classList.remove('visible');
            });
        });
    </script>
</body>
</html>`;
    }

    /**
     * Static version of layout calculation for standalone HTML
     */
    private static _calculateLayoutStatic(nodes: QvernNode[]): Map<string, { x: number; y: number; level: number }> {
        const positions = new Map<string, { x: number; y: number; level: number }>();

        if (nodes.length === 0) return positions;

        const children = new Map<string, string[]>();
        const parentsMap = new Map<string, string[]>();
        const nodeIds = new Set(nodes.map(n => n.nodeId));

        nodes.forEach(n => {
            const validParents = n.parents.filter(p => nodeIds.has(p));
            parentsMap.set(n.nodeId, validParents);
            validParents.forEach(p => {
                if (!children.has(p)) children.set(p, []);
                children.get(p)!.push(n.nodeId);
            });
        });

        const levels = new Map<string, number>();

        const calculateLevel = (nodeId: string, visited: Set<string>): number => {
            if (levels.has(nodeId)) return levels.get(nodeId)!;
            if (visited.has(nodeId)) return 0;

            visited.add(nodeId);
            const nodeParents = parentsMap.get(nodeId) || [];

            if (nodeParents.length === 0) {
                levels.set(nodeId, 0);
                return 0;
            }

            let maxParentLevel = -1;
            for (const parent of nodeParents) {
                const parentLevel = calculateLevel(parent, visited);
                maxParentLevel = Math.max(maxParentLevel, parentLevel);
            }

            const level = maxParentLevel + 1;
            levels.set(nodeId, level);
            return level;
        };

        nodes.forEach(n => calculateLevel(n.nodeId, new Set()));

        const levelGroups = new Map<number, string[]>();
        levels.forEach((level, id) => {
            if (!levelGroups.has(level)) levelGroups.set(level, []);
            levelGroups.get(level)!.push(id);
        });

        const nodeMap = new Map(nodes.map(n => [n.nodeId, n]));
        levelGroups.forEach((ids) => {
            ids.sort((a, b) => {
                const nodeA = nodeMap.get(a);
                const nodeB = nodeMap.get(b);
                if (nodeA?.createdAt && nodeB?.createdAt) {
                    return new Date(nodeA.createdAt).getTime() - new Date(nodeB.createdAt).getTime();
                }
                return a.localeCompare(b);
            });
        });

        const nodeWidth = 140;
        const nodeHeight = 50;
        const levelSpacing = 100;
        const nodeSpacing = 30;

        levelGroups.forEach((ids, level) => {
            const startX = 50;

            ids.forEach((id, idx) => {
                positions.set(id, {
                    x: startX + idx * (nodeWidth + nodeSpacing),
                    y: 30 + level * (nodeHeight + levelSpacing),
                    level
                });
            });
        });

        return positions;
    }

    /**
     * Static version of graph rendering for standalone HTML
     */
    private static _renderGraphStatic(nodes: QvernNode[], positions: Map<string, { x: number; y: number; level: number }>): string {
        if (nodes.length === 0) {
            return '<p style="color: #6c7086; text-align: center;">No nodes in graph</p>';
        }

        const nodeWidth = 140;
        const nodeHeight = 50;

        let maxX = 0, maxY = 0;
        positions.forEach(pos => {
            maxX = Math.max(maxX, pos.x + nodeWidth);
            maxY = Math.max(maxY, pos.y + nodeHeight);
        });

        const svgWidth = maxX + 50;
        const svgHeight = maxY + 50;

        const edges: string[] = [];
        nodes.forEach(node => {
            const pos = positions.get(node.nodeId);
            if (!pos) return;

            node.parents.forEach(parentId => {
                const parentPos = positions.get(parentId);
                if (!parentPos) return;

                const startX = parentPos.x + nodeWidth / 2;
                const startY = parentPos.y + nodeHeight;
                const endX = pos.x + nodeWidth / 2;
                const endY = pos.y;

                const midY = (startY + endY) / 2;
                const isSpineEdge = node.status === 'spine';

                edges.push(`
                    <path class="edge ${isSpineEdge ? 'to-spine' : ''}" 
                          d="M ${startX} ${startY} C ${startX} ${midY}, ${endX} ${midY}, ${endX} ${endY}" />
                `);
            });
        });

        const nodeSvg: string[] = [];
        nodes.forEach(node => {
            const pos = positions.get(node.nodeId);
            if (!pos) return;

            const shortId = node.nodeId.substring(0, 9);
            const badges: string[] = [];

            if (node.isHead) {
                badges.push(`<text class="node-badge head-badge" x="${pos.x + nodeWidth - 8}" y="${pos.y + 12}" text-anchor="end">HEAD</text>`);
            }
            if (node.status === 'spine') {
                badges.push(`<text class="node-badge spine-badge" x="${pos.x + nodeWidth - 8}" y="${pos.y + nodeHeight - 8}" text-anchor="end">SPINE</text>`);
            }

            nodeSvg.push(`
                <g class="node-group" data-node-id="${node.nodeId}">
                    <rect class="node-rect ${node.status}" 
                          x="${pos.x}" y="${pos.y}" 
                          width="${nodeWidth}" height="${nodeHeight}" />
                    <text class="node-id" x="${pos.x + 10}" y="${pos.y + 22}">${shortId}</text>
                    <text class="node-status" x="${pos.x + 10}" y="${pos.y + 38}">${node.zone} · ${node.status}</text>
                    ${badges.join('')}
                </g>
            `);
        });

        return `
            <svg class="graph-svg" width="${svgWidth}" height="${svgHeight}" viewBox="0 0 ${svgWidth} ${svgHeight}">
                <defs>
                    <marker id="arrowhead" markerWidth="10" markerHeight="7" 
                            refX="9" refY="3.5" orient="auto">
                        <polygon points="0 0, 10 3.5, 0 7" fill="#45475a" />
                    </marker>
                </defs>
                <g class="edges">${edges.join('')}</g>
                <g class="nodes">${nodeSvg.join('')}</g>
            </svg>
        `;
    }
}

