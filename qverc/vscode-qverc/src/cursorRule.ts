/**
 * Cursor Rule Management
 *
 * Generates and maintains a .cursor/rules/qverc.mdc file so that
 * AI agents in Cursor automatically understand the qverc workflow.
 */

import * as fs from 'fs';
import * as path from 'path';

const RULE_VERSION = 'qverc-rule-v2';
const RULE_DIR = '.cursor/rules';
const RULE_FILE = 'qverc.mdc';
const SENTINEL = `<!-- managed:${RULE_VERSION} -->`;

const RULE_CONTENT = `---
description: Qverc version control system - commands and workflow for AI agents
alwaysApply: true
---
${SENTINEL}

# This project uses qverc, not git

qverc is a DAG-based version control system for AI agent workflows.
Do NOT use git commands for version control. Use qverc commands instead.

## REQUIRED: follow this workflow for every change

You MUST follow these steps whenever you make code changes:

1. **Before starting**: run \`qverc edit "description of what you will change"\`
2. **Make your code changes** as needed
3. **When done**: run \`qverc sync --agent "your-model-name" --skip-verify\`

If the user asks you to promote, run \`qverc promote --skip-verify --force\`.

Never skip the edit/sync steps. Never use git add, git commit, or git push.
Always pass \`--agent\` to identify which model produced the changes.

## Checking state

Run \`qverc status\` to see the current node, intent, and changed files.
Run \`qverc log --all\` to see the full DAG history.

## Key commands

| Command | Purpose |
|---------|---------|
| \`qverc edit "intent"\` | Declare what you are about to change |
| \`qverc sync --agent "model"\` | Snapshot, verify, and commit to the DAG |
| \`qverc status\` | Show current node, intent, and changed files |
| \`qverc log [--all]\` | Display DAG history |
| \`qverc checkout <node> [-f]\` | Restore workspace to a previous state |
| \`qverc promote [--skip-verify --force]\` | Promote a node to the Spine |
| \`qverc prune --older-than 7d --execute\` | Garbage-collect old exploration nodes |
| \`qverc merge run <n1> <n2> -i "intent"\` | Merge two nodes |
| \`qverc squash <start> <end>\` | Squash a linear node sequence |

## Merge workflow

All versions of conflicting files are placed in \`.qverc/merge/files/\`.
Resolve by writing final files to the workspace, then \`qverc sync\`.
No conflict markers are used.

## Verification tiers (Gatekeeper)

Configured in \`qverc.toml\`:
- **Tier 1** - syntax / linter (Draft -> Valid)
- **Tier 2** - unit tests (Valid -> Verified)
- **Tier 3** - full integration (Verified -> Spine, runs on \`qverc promote\`)

## Configuration

Project settings live in \`qverc.toml\`. Ignore patterns go in \`.qvignore\`.
`;

/**
 * Ensure the .cursor/rules/qverc.mdc file exists and is up-to-date.
 * Skips writing if the file exists and was manually edited (sentinel removed).
 */
export async function ensureCursorRule(workspaceRoot: string): Promise<void> {
    const ruleDir = path.join(workspaceRoot, RULE_DIR);
    const rulePath = path.join(ruleDir, RULE_FILE);

    if (fs.existsSync(rulePath)) {
        const existing = fs.readFileSync(rulePath, 'utf-8');

        if (!existing.includes('managed:qverc-rule-v')) {
            return;
        }

        if (existing.includes(SENTINEL)) {
            return;
        }
    }

    fs.mkdirSync(ruleDir, { recursive: true });
    fs.writeFileSync(rulePath, RULE_CONTENT, 'utf-8');
}
