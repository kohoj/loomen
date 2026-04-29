# Roadmap

Loomen should grow as a code-evolution instrument: local-first, evidence-rich, visually calm, and operationally precise. The roadmap is organized around making each Loomen verb more trustworthy.

## Weave Better Paths

- Add first-run dependency checks for Git, Bun, Claude Code, Codex, and GitHub CLI.
- Make workspace creation explain the resulting git state before it runs: base branch, new branch, worktree path, and checkpoint baseline.
- Add workspace templates for common setup/run scripts.
- Add quick workspace creation from selected file, diff, PR, search result, or prompt.
- Add resource governance for newly woven paths: disk usage estimate, stale workspace warnings, and cleanup previews.

## Ray More Evidence

- Add sibling workspace comparison, not only checkpoint-to-current diffs.
- Add richer review threads with unresolved comment filters.
- Add PR timeline and checks drilldown.
- Add CI log summaries for failed checks.
- Add clearer context-usage accounting from provider-specific usage events.
- Add more explicit spotlighter status, including conflicts and pending mirror previews.

## Beam Clearer Sessions

- Improve SDK-native event typing beyond CLI JSONL parsing.
- Store structured session events separately from rendered transcript text.
- Implement first-class UI for interactive agent questions instead of returning skipped answers.
- Expand approval UI for multi-step tool calls and longer plan reviews.
- Add graceful sidecar restart and clearer runtime diagnostics.
- Add per-provider environment profiles.

## Pulse Reliable Validation

- Make Pulse evidence feed Fuse readiness and workspace comparison views.
- Expand named validation pulses with user-defined aliases and richer framework detection.
- Add future multi-workspace Pulse to run the same validation across sibling paths.
- Add pass/fail summaries that can feed Fuse readiness.
- Add safer long-running process controls for dev servers and watch commands.

## Fuse With Confidence

- Feed merge readiness from richer evidence sources such as unresolved risks, multi-workspace Pulse, and conflict previews.
- Add conflict-aware merge previews before any target-branch mutation.
- Add merge/automerge flows after PR success.
- Add explicit handoff states: draft, ready for review, ready to fuse, fused.
- Keep semantic merge assistance advisory; the user remains the final decision maker.

## Sever Without Regret

- Add safer archive cleanup execution with optional branch/worktree deletion.
- Add cleanup confirmation that reuses the preview and requires explicit final consent.
- Add restore checks that verify the worktree path and branch still exist.
- Add stale workspace nudges instead of automatic deletion.
- Keep destructive actions explicit and reversible until the final cleanup step.

## Product Atmosphere

- Bring Loomen vocabulary into visible and invisible surfaces: buttons, command palette actions, empty states, notifications, settings descriptions, docs, tests, and event names.
- Use the rule "poetic action, plain object": Weave a workspace, Ray a path, Beam a session, Pulse tests, Fuse through review, Sever archived work.
- Tighten dense macOS spacing and keyboard workflows without turning the product into a generic enterprise dashboard.
- Use visual language that feels like light, glass, paths, signals, and instruments, not heavy sci-fi decoration.
- Add stronger empty states that teach the workbench model without returning to CLI-era terminology.

## Architecture Health

- Continue splitting `src-tauri/src/main.rs` into domain modules; Pulse command discovery/evidence, PTY terminal sessions/snapshots, diff review parsing/comments, and GitHub PR/check operations now live behind Rust module boundaries, with persistence, git, sidecar, settings, and app bootstrap still to follow.
- Split `src/main.ts` into state, rendering, command handlers, agent/session UI, files, changes, terminal, notifications, and settings modules.
- Add schema migration tests for existing user databases.
- Add sidecar protocol fixtures for Claude and Codex JSONL variants.
- Keep `dist/` reproducible and checked in only as the Tauri runtime frontend artifact.

## Distribution

- Ship a packaged app bundle with signing and notarization configuration.
- Add a launch health screen for database path, sidecar socket, agent binary discovery, and `gh` availability.
- Add export/import for local settings.
- Keep private databases, app bundles, generated source maps, native build output, and local research notes out of git.
