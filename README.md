# Loomen

Weave logic, seek lumen.

AI coding changes software from a single line of edits into many candidate futures. Loomen is a local macOS workbench that gives each future a real place to live, a way to be seen, and a path back home.

Every Loomen path is grounded in ordinary git: a repository, a branch, a worktree, sessions, checkpoints, diffs, terminals, and review evidence. The surface is poetic; the machinery stays inspectable.

Loomen is a Tauri 2 app with a Rust backend, a static WebView frontend, SQLite persistence, PTY terminals, git workspace management, and a Unix-socket JSON-RPC sidecar for Claude Code / Codex CLI execution.

## The Loomen Model

- **Weave** a workspace: create a task-scoped path from a base branch using a real `git worktree add` branch.
- **Ray** a path: reveal files, diffs, search results, PR/check state, context usage, and terminal evidence.
- **Beam** a session: watch Claude Code or Codex stream through a path with messages, events, and approvals.
- **Pulse** validation: run setup scripts, test scripts, shell commands, and future multi-workspace checks.
- **Fuse** through review: checkpoint, inspect, comment, publish PRs, follow checks, and prepare a path for merge.
- **Sever** with care: archive, restore, and eventually clean up branches and worktrees only when the user chooses.

These words are not decorative labels. They are the operating verbs of a code-evolution instrument: create paths, illuminate them, observe their live flow, test them, bring the worthy ones home, and stop carrying the rest.

## Current Surface

- Import local git repositories and inspect branch / remote metadata.
- Weave task workspaces backed by real `git worktree add` branches.
- Beam Claude Code or Codex sessions from the app with streaming output, cancellation, permission modes, and approximate context usage.
- Ray workspace state through file trees, safe previews, search, structured patches, hunk navigation, line comments, Finder actions, PR status, and check state.
- Pulse setup scripts, run scripts, one-off shell commands, PTY-backed zsh terminals, and recent validation evidence per workspace.
- Fuse work through non-destructive checkpoint refs under `refs/loomen-checkpoints/<id>`, diff review, comments, draft PR creation/update, and check reruns.
- Sever work through archive/restore flows today, with more explicit branch and worktree cleanup planned.
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

The checked-in `dist/` directory is intentional: it is the frontend used by Tauri for the current implementation. No npm install is needed to run the app as checked in.

## Data

Loomen stores its local SQLite database in the macOS application data directory for `dev.kohoj.loomen` as `loomen.db`.

Build artifacts under `src-tauri/target/` are intentionally ignored and should not be committed.

## Repository Status

The repository is source-available application code and currently does not include an open-source license. It also does not include proprietary binaries, private databases, or vendored Claude/Codex executables.

## Docs

- [Semantics](docs/SEMANTICS.md): the product language charter for Weave, Ray, Beam, Pulse, Fuse, and Sever.
- [Status](docs/STATUS.md): current implementation state, known gaps, and verification.
- [Roadmap](docs/ROADMAP.md): next steps for product direction, runtime, delivery, and architecture health.
