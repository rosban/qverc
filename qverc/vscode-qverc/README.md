# qverc VS Code Extension

Visual Studio Code / Cursor extension for [qverc](https://github.com/rosban/qverc) - Quantum Version Control for AI workflows.

## Features

### File Status Decorations

Files in the Explorer show their qverc status with colored badges:

- **A** (Green) - Added: New file not tracked in current node
- **M** (Yellow) - Modified: File changed from current node
- **D** (Red) - Deleted: File removed (shown on checkout)

<!-- File decoration badges appear in the file explorer -->

### Status Bar

The status bar shows the current qverc state:

```
⭐ qv-abc123 (verified) +2
```

- Node ID with truncated hash
- Current status (draft/valid/verified/spine)
- Number of uncommitted changes

Click the status bar to access quick actions.

### Commands

Access from Command Palette (`Ctrl+Shift+P` / `Cmd+Shift+P`):

| Command | Description |
|---------|-------------|
| `qverc: Initialize Repository` | Run `qverc init` |
| `qverc: Sync` | Commit changes to the graph |
| `qverc: Set Intent` | Set editing intent for next sync |
| `qverc: Checkout Node` | Switch to a different node |
| `qverc: Show Status` | Display current status |
| `qverc: Show Log` | Display DAG history |
| `qverc: Refresh Status` | Manually refresh decorations |

## Installation

### From Source

1. Clone the qverc repository:
   ```bash
   git clone https://github.com/qverc/qverc.git
   cd qverc/vscode-qverc
   ```

2. Install dependencies:
   ```bash
   npm install
   ```

3. Compile:
   ```bash
   npm run compile
   ```

4. Package the extension:
   ```bash
   npx vsce package
   ```

5. Install in VS Code:
   - Open VS Code
   - Go to Extensions (`Ctrl+Shift+X`)
   - Click `...` menu → `Install from VSIX...`
   - Select the generated `.vsix` file

### Development

1. Open `vscode-qverc` folder in VS Code
2. Press `F5` to launch Extension Development Host
3. Make changes and reload (`Ctrl+R`) to test

## Configuration

Open Settings (`Ctrl+,`) and search for "qverc":

| Setting | Default | Description |
|---------|---------|-------------|
| `qverc.executablePath` | `"qverc"` | Path to qverc executable |
| `qverc.autoRefresh` | `true` | Auto-refresh status on file changes |
| `qverc.refreshInterval` | `5000` | Refresh interval in milliseconds |

### Custom Executable Path

If qverc is not in your PATH:

```json
{
  "qverc.executablePath": "/path/to/qverc"
}
```

## Requirements

- **qverc CLI** must be installed and accessible
- VS Code 1.80.0 or later

## Theming

The extension defines custom colors that adapt to your theme:

- `qverc.addedForeground` - Color for added files
- `qverc.modifiedForeground` - Color for modified files
- `qverc.deletedForeground` - Color for deleted files

Override in your `settings.json`:

```json
{
  "workbench.colorCustomizations": {
    "qverc.addedForeground": "#00ff00",
    "qverc.modifiedForeground": "#ffff00"
  }
}
```

## Troubleshooting

### Extension not activating

- Ensure the workspace contains a `.qverc/` directory
- Check that qverc is installed: `qverc --version`

### Status not updating

- Run `qverc: Refresh Status` manually
- Check the Output panel (`View → Output → qverc`)
- Verify `qverc.executablePath` is correct

### Permission errors

- On Unix, ensure qverc is executable: `chmod +x /path/to/qverc`

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make changes
4. Test with F5 debug
5. Submit a pull request

## License

MIT License - See [LICENSE](https://github.com/qverc/blob/main/LICENSE) for details.

