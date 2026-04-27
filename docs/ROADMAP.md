# Roadmap

## Runtime

- Ship a packaged app bundle with signing/notarization configuration.
- Add graceful sidecar restart and clearer runtime diagnostics.
- Add first-run dependency checks for Git, Bun, Claude, Codex, and GitHub CLI.

## Agent Integration

- Improve SDK-native event typing beyond CLI JSONL parsing.
- Add richer token/context accounting from provider-specific usage events.
- Expand approval UI for multi-step tool calls and longer plan reviews.
- Add per-provider environment profiles.

## Workspace Lifecycle

- Add safer archive cleanup with optional branch/worktree deletion.
- Add merge/automerge flows after PR success.
- Add conflict-aware spotlighter status and clearer mirror previews.
- Add workspace templates for common setup/run scripts.

## Product Surface

- Continue tightening keyboard workflows and dense macOS spacing.
- Add richer review threads, unresolved comment filters, and checks drilldown.
- Add command palette grouping for recent files, workspaces, scripts, and PR actions.
- Add export/import for local settings.
