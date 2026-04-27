# Status

Loomen is currently a runnable macOS Tauri app with real local persistence and git operations.

## Implemented

- Tauri 2 desktop shell with static WebView frontend.
- SQLite tables for repositories, workspaces, sessions, messages, settings, terminal snapshots, lifecycle runs, and diff comments.
- Real repository import through `git rev-parse`, remote/default-branch discovery, and branch enumeration.
- Real workspace creation with `git worktree add` and configurable branch/worktree naming.
- Non-destructive checkpoint commits written to `refs/loomen-checkpoints/<id>` through a temporary git index.
- Claude and Codex CLI adapters in the TypeScript sidecar, selected by session agent type.
- Newline-delimited JSON-RPC over a Unix domain socket between Rust and the sidecar.
- Streaming assistant messages, session events, cancellation, and tool-approval prompts.
- Reverse RPC handlers for diff, comments, terminal output, plan mode, user questions, and tool approval.
- Settings pages for models, providers, appearance, git defaults, account placeholders, experiments, and advanced paths.
- Dark workbench UI with repository/history sidebar, workspace tabs, Scratchpad, chat sessions, command palette, notifications, composer controls, slash commands, and file mentions.
- All-files inspector with collapsible tree, workspace search, safe file preview, line highlighting, copy/open/reveal actions, binary detection, and large-file truncation.
- Changes inspector with structured patch parsing, hunk navigation, additions/deletions summary, changed-file filter, patch copy, Finder reveal, and line-aware diff comments.
- GitHub PR/check panel via `gh pr view`, `gh pr create`, `gh pr edit`, and `gh run rerun --failed`.
- Setup/run scripts plus PTY-backed zsh terminal tabs with persisted scrollback snapshots.
- Local spotlighter script that mirrors changed workspace files back to the root repository while enabled.

## Verification

Use:

```bash
node --check dist/main.js
cargo test --manifest-path src-tauri/Cargo.toml
./script/build_and_run.sh --verify
```

The GitHub PR write actions are intentionally not covered by automated tests because they modify remote state.

## Publish Hygiene

- No build output should be committed.
- No app bundle, private database, binary assets, or local research notes are required.
- Agent executables are discovered from explicit settings, `LOOMEN_CLAUDE_BIN` / `LOOMEN_CODEX_BIN`, or `PATH`.
