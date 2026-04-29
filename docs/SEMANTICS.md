# Semantics

Loomen's language starts from a product belief: AI coding creates many possible futures for a codebase, and those futures need more than chat. They need places, light, signals, judgment, and endings.

Loomen should feel like a code-evolution instrument: local-first, precise, visually calm, and quietly luminous. Its product vocabulary is poetic, but its engineering body stays plain.

## First Principles

AI changes the shape of development. The important shift is not that a developer can ask a model for edits. The shift is that many credible paths can be explored at once.

That creates a new product requirement. A candidate future needs:

- a real place to exist without corrupting other paths
- enough light to understand what changed and why
- live observation while an agent works
- repeatable validation signals
- a careful route back into the main codebase
- a clean end when it no longer deserves attention

Loomen's verbs are the primitives for that work: **Weave**, **Ray**, **Beam**, **Pulse**, **Fuse**, and **Sever**.

## User Experience

The rule is:

> poetic action, plain object.

Loomen words should be visible in product and docs, but not at the cost of orientation. A user should see the brand language and immediately understand the effect:

- Weave a workspace
- Ray a path
- Beam a session
- Pulse tests
- Fuse through review
- Sever archived work

The verb carries the identity. The object explains the operation. Engineering terms such as repository, workspace, worktree, branch, session, checkpoint, diff, PR, and archive remain necessary because they keep the system inspectable.

## Brand Tone

Loomen should not feel like a generic agent IDE, a task dashboard, or a chat wrapper. It should feel like a precise instrument built by someone who cares about light, motion, texture, and evidence.

The visual and verbal tone should be:

- quiet rather than loud
- luminous rather than neon
- exact rather than mystical
- local and material rather than cloudy and abstract
- generative, but never vague

The product can use optical and weaving language, but should avoid turning every noun into metaphor. Git is still git. A worktree is still a worktree. A PR is still a PR.

## Verbs

### Weave

Meaning: create a candidate path.

User-facing body: workspace creation from a repository and base branch.

Engineering body: `git worktree add`, branch naming, SQLite workspace record, checkpoint baseline.

Restraint: Weave should not hide that it creates real git state.

### Ray

Meaning: make a path's state visible.

User-facing body: file tree, preview, search, changes, patch hunks, comments, checks, PR status, and context usage.

Engineering body: filesystem reads, `git status`, `git diff`, patch parsing, `gh pr view`, `gh run` data, stored diff comments.

Restraint: Ray should reveal evidence, not summarize away details the user needs to judge.

### Beam

Meaning: observe the live flow inside a path.

User-facing body: Claude Code and Codex sessions, streaming messages, tool events, approvals, cancellation, and transcript history.

Engineering body: TypeScript sidecar, newline-delimited JSON-RPC over a Unix socket, CLI JSONL parsing, session messages, session events.

Restraint: Beam should make agent activity legible without pretending the agent is deterministic or magical.

### Pulse

Meaning: send a signal or validation action through one or more paths.

User-facing body: setup scripts, run scripts, shell commands, PTY terminals, and future multi-workspace test broadcasts.

Engineering body: zsh execution, PTY sessions, terminal snapshots, lifecycle logs, persisted run records.

Restraint: Pulse should be repeatable and attributable. The user should know what ran, where it ran, and what it proved.

### Fuse

Meaning: bring a proven path back home.

User-facing body: checkpointing, diff review, comments, PR creation/update, check status, reruns, merge readiness, and future merge execution.

Engineering body: `refs/loomen-checkpoints/<id>`, temporary git indexes, structured patch parsing, GitHub CLI calls, review comments, CI status.

Restraint: Fuse should not promise automatic semantic merging. It is a decision pipeline, not a magic merge button.

### Sever

Meaning: stop carrying a path.

User-facing body: archive, restore, cleanup previews, and optional branch/worktree deletion.

Engineering body: workspace state, archive metadata, future git branch deletion, future `git worktree remove`, database updates.

Restraint: Sever should be explicit and reversible until the user chooses destructive cleanup.

## Boundaries

- Loomen is local-first. Repositories, worktrees, transcripts, checkpoints, and the SQLite database live on the user's machine unless the user explicitly uses GitHub features.
- Loomen does not vendor agent CLIs. It discovers Claude Code and Codex from settings, `LOOMEN_CLAUDE_BIN` / `LOOMEN_CODEX_BIN`, or `PATH`.
- Loomen does not promise automatic semantic merging. Human review remains central to Fuse.
- Loomen is source-available application code and currently ships without an open-source license.
- Loomen should preserve engineering clarity. Brand language may guide the product surface, but implementation docs must remain operationally precise.
