# Loomen

Loomen is a local macOS workbench for running agent-backed coding sessions in isolated git worktrees.

It is a Tauri 2 desktop app with a Rust backend, a static WebView frontend, SQLite persistence, PTY terminals, git workspace management, and a Unix-socket JSON-RPC sidecar for Claude Code / Codex CLI execution.

## Features

- Import local git repositories and inspect branch / remote metadata.
- Create task workspaces backed by real `git worktree add` branches.
- Run Claude Code or Codex sessions from the app, with streaming output, cancellation, permission modes, and approximate context usage.
- Review workspace changes with structured patches, hunk navigation, line comments, file preview, search, and Finder actions.
- Save non-destructive checkpoint refs under `refs/loomen-checkpoints/<id>`.
- Manage setup scripts, run scripts, and PTY-backed zsh terminals per workspace.
- Read GitHub PR/check status through `gh`, create or update draft PRs, and rerun failed checks.
- Keep per-workspace scratchpad notes, local notifications, settings, command palette entries, slash commands, and `@file` suggestions.

## Requirements

- macOS
- Rust stable
- Bun, for the TypeScript sidecar (`bun sidecar/index.ts`)
- Git
- Optional: `gh`, `claude`, and `codex` on `PATH`

The app does not vendor agent CLIs. Configure custom executable paths in Settings, or set:

```bash
export LOOMEN_CLAUDE_BIN=/path/to/claude
export LOOMEN_CODEX_BIN=/path/to/codex
```

## Run

```bash
./script/build_and_run.sh
```

Verify that the desktop process launches:

```bash
./script/build_and_run.sh --verify
```

Build or test directly:

```bash
cargo build --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
```

Sidecar protocol harness:

```bash
bun sidecar/index.ts
```

The checked-in `dist/` directory is the frontend used by Tauri. No npm install is needed for the current implementation.

## Data

Loomen stores its local SQLite database in the macOS application data directory for `dev.kohoj.loomen` as `loomen.db`.

Build artifacts under `src-tauri/target/` are intentionally ignored and should not be committed.
