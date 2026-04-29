# Status

Loomen is currently a runnable macOS Tauri workbench with real local persistence, real git worktree operations, local agent execution, and a dense desktop review surface.

The implementation is already past the CLI prototype stage. It is a local desktop app with a Rust backend, SQLite state, a static WebView frontend, PTY terminals, git operations, GitHub CLI integration, and a TypeScript sidecar for Claude Code / Codex execution.

## Implemented

### Weave

- Local repository import through `git rev-parse`.
- Remote/default-branch discovery and branch enumeration.
- Workspace creation with real `git worktree add`.
- Configurable branch naming and worktree path generation.
- SQLite records for repositories, workspaces, sessions, messages, settings, terminal snapshots, lifecycle runs, and diff comments.
- Initial non-destructive checkpoint refs for workspaces.

### Ray

- All-files inspector with collapsible tree navigation.
- Safe file preview with binary detection, large-file truncation, line highlighting, copy/open/reveal actions, and Finder integration.
- Workspace content search backed by command-line search.
- Changes inspector with structured patch parsing, hunk navigation, additions/deletions summary, changed-file filtering, patch copy, Finder reveal, and line-aware diff comments.
- GitHub PR/check panel through `gh pr view`, check rollup parsing, and check rerun support.
- Approximate context usage surfaced for sessions.

### Beam

- Claude Code and Codex CLI adapters in the TypeScript sidecar.
- Session agent type selection for Claude or Codex.
- Newline-delimited JSON-RPC over a Unix domain socket between Rust and the sidecar.
- Streaming assistant messages from agent output.
- Forwarded session events for non-text tool/activity events.
- Cancellation and tool-approval prompts.
- Reverse RPC handlers for diff, comments, terminal output, plan mode exit, skipped interactive user questions, and tool approval.

### Pulse

- Named validation pulses discovered from `package.json` scripts and Cargo manifests.
- Repository setup scripts.
- Repository run scripts.
- One-off workspace shell command execution.
- PTY-backed zsh terminal tabs per workspace.
- Persisted terminal scrollback snapshots.
- Lifecycle logs for setup and run activity.
- Pulse evidence records for setup scripts, run scripts, and one-off commands, including label, kind, exit status, duration, workspace path, output, and checkpoint attribution.
- Local spotlighter script that mirrors changed workspace files back to the root repository while enabled.

### Fuse

- Non-destructive checkpoint commits written to `refs/loomen-checkpoints/<id>` through a temporary git index.
- Diff review against checkpoint refs.
- Diff comments stored per workspace/file/line.
- Draft PR creation and update through `gh pr create` and `gh pr edit`.
- PR/check status reading through `gh`.
- Fuse readiness snapshot that combines checkpoint presence, Pulse evidence, unresolved diff comments, and PR/check state.
- Failed-check reruns through `gh run rerun --failed`.

### Sever

- Workspace archive and restore state.
- Archive metadata storage.
- Cleanup preview for branch, worktree, lifecycle logs, terminal evidence, terminal tabs, sessions, diff comments, and database record counts.
- Conservative cleanup posture: destructive branch/worktree deletion is not automatic.

### Foundation

- Tauri 2 desktop shell with static WebView frontend.
- Rust backend with command handlers for repositories, workspaces, sessions, files, diffs, terminals, PRs, settings, and sidecar lifecycle.
- Pulse backend logic is split into a dedicated Rust module for named validation discovery, command execution, evidence storage, and labels.
- PTY terminal lifecycle, scrollback capture, and terminal tab snapshots are split into a dedicated Rust module.
- Diff review parsing and diff comment storage are split into a dedicated Rust module.
- GitHub PR/check CLI operations and check-rollup parsing are split into a dedicated Rust module.
- Sidecar process startup, socket RPC, cancellation, streaming query handling, and message extraction are split into a dedicated Rust module.
- Git root resolution, branch discovery, worktree creation, checkpoint refs, and checkpoint diffs are split into a dedicated Rust module.
- Settings schema, default values, SQLite persistence, parser tolerance, and session defaults are split into a dedicated Rust module.
- Database schema creation and compatibility migrations are split into a dedicated Rust module.
- Dark workbench UI with repository/history sidebar, workspace tabs, Scratchpad, chat sessions, command palette, notifications, composer controls, slash commands, and file mentions.
- Settings pages for models, providers, appearance, git defaults, account placeholders, experiments, and advanced paths.

## Known Gaps

- The Rust command layer and frontend entrypoint are still large; Pulse, terminal, review, GitHub, sidecar, Git, settings, and database schema now have module boundaries, while the remaining persistence queries and app bootstrap still need extraction.
- Interactive agent questions are acknowledged but currently skipped rather than rendered as first-class UI.
- Sidecar diagnostics and restart behavior need to be more explicit.
- Merge/archive cleanup is intentionally conservative and still needs an execution flow for branch deletion and `git worktree remove`.
- `dist/` is checked in intentionally for Tauri, but generated source maps and native build output should stay out of git.

## Verification

Use:

```bash
node --check dist/main.js
cargo test --manifest-path src-tauri/Cargo.toml
./script/build_and_run.sh --verify
```

The GitHub PR write actions are intentionally not covered by automated tests because they modify remote state.

## Publish Hygiene

- `dist/` is intentionally committed for the current Tauri frontend.
- Native build output, app bundles, private databases, binary assets, and local research notes should not be committed.
- Agent executables are discovered from explicit settings, `LOOMEN_CLAUDE_BIN` / `LOOMEN_CODEX_BIN`, or `PATH`.
