# Loomen Product Direction Docs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rewrite Loomen's public docs so they define the product from first principles, user experience, and brand tone instead of mechanically mapping old CLI terms to the new workbench.

**Architecture:** This is a documentation-only change. `README.md` becomes the product entry point, `docs/SEMANTICS.md` becomes the language and brand charter, `docs/STATUS.md` becomes the factual implementation inventory organized by Loomen verbs, and `docs/ROADMAP.md` becomes the product direction map.

**Tech Stack:** Markdown docs, existing Tauri/Rust/Bun project, verification with `node --check dist/main.js` and `cargo test --manifest-path src-tauri/Cargo.toml`.

---

## File Structure

- Modify `README.md`: product vision, six first-class Loomen verbs, current capabilities, run instructions, docs index.
- Modify `docs/SEMANTICS.md`: first-principles model, user experience rule, brand tone, Loomen verb semantics, boundaries.
- Modify `docs/STATUS.md`: factual current state grouped by Weave/Ray/Beam/Pulse/Fuse/Sever plus known gaps and verification.
- Modify `docs/ROADMAP.md`: future direction grouped by the six verbs, product quality, architecture health, and distribution.

---

### Task 1: Rewrite README As Product Entry Point

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Replace the opening with first-principles positioning**

Use this opening:

```markdown
# Loomen

Weave logic, seek lumen.

AI coding changes software from a single line of edits into many candidate futures. Loomen is a local macOS workbench that gives each future a real place to live, a way to be seen, and a path back home.

Every Loomen path is grounded in ordinary git: a repository, a branch, a worktree, sessions, checkpoints, diffs, terminals, and review evidence. The surface is poetic; the machinery stays inspectable.
```

- [ ] **Step 2: Add the six product verbs**

Insert a section named `## The Loomen Model` with exactly these bullets:

```markdown
## The Loomen Model

- **Weave** a workspace: create a task-scoped path from a base branch using a real `git worktree add` branch.
- **Ray** a path: reveal files, diffs, search results, PR/check state, context usage, and terminal evidence.
- **Beam** a session: watch Claude Code or Codex stream through a path with messages, events, and approvals.
- **Pulse** validation: run setup scripts, test scripts, shell commands, and future multi-workspace checks.
- **Fuse** through review: checkpoint, inspect, comment, publish PRs, follow checks, and prepare a path for merge.
- **Sever** with care: archive, restore, and eventually clean up branches and worktrees only when the user chooses.
```

- [ ] **Step 3: Keep run/data/repository sections concise**

Keep requirements, run commands, data location, and repository status. Remove any wording that frames the workbench as a replacement table for old CLI commands.

- [ ] **Step 4: Review README manually**

Run:

```bash
sed -n '1,220p' README.md
```

Expected: the README starts from the multi-future code evolution idea, names all six verbs, and still tells a user how to run the app.

---

### Task 2: Rewrite SEMANTICS As Language Charter

**Files:**
- Modify: `docs/SEMANTICS.md`

- [ ] **Step 1: Replace translation-table framing**

Start the document with:

```markdown
# Semantics

Loomen's language starts from a product belief: AI coding creates many possible futures for a codebase, and those futures need more than chat. They need places, light, signals, judgment, and endings.
```

- [ ] **Step 2: Add the three design lenses**

Add sections:

```markdown
## First Principles
## User Experience
## Brand Tone
```

Each section must describe the product without saying "old X maps to new Y".

- [ ] **Step 3: Define each verb with body and restraint**

For every verb, include:

```markdown
### Weave

Meaning: create a candidate path.

User-facing body: workspace creation from a repository and base branch.

Engineering body: `git worktree add`, branch naming, SQLite workspace record, checkpoint baseline.

Restraint: Weave should not hide that it creates real git state.
```

Repeat the same structure for `Ray`, `Beam`, `Pulse`, `Fuse`, and `Sever`.

- [ ] **Step 4: Add boundaries**

Include boundaries for local-first data, no vendored agent CLIs, no automatic semantic merge promise, and no open-source license.

- [ ] **Step 5: Review SEMANTICS manually**

Run:

```bash
sed -n '1,260p' docs/SEMANTICS.md
```

Expected: the document reads as a language charter, not as parity notes.

---

### Task 3: Rewrite STATUS Around Implemented Verbs

**Files:**
- Modify: `docs/STATUS.md`

- [ ] **Step 1: Keep factual opening**

Use:

```markdown
# Status

Loomen is currently a runnable macOS Tauri workbench with real local persistence, real git worktree operations, local agent execution, and a dense desktop review surface.
```

- [ ] **Step 2: Group implementation by verb**

Create `## Implemented` with subsections:

```markdown
### Weave
### Ray
### Beam
### Pulse
### Fuse
### Sever
### Foundation
```

Place current factual capabilities under these headings. Keep implementation names exact: Tauri 2, Rust, SQLite, Unix socket JSON-RPC, `gh`, PTY, `refs/loomen-checkpoints/<id>`.

- [ ] **Step 3: Keep known gaps and verification**

Keep `## Known Gaps`, `## Verification`, and `## Publish Hygiene`. Ensure gaps mention interactive user questions, large files needing module boundaries, sidecar diagnostics, and merge/archive completion.

- [ ] **Step 4: Review STATUS manually**

Run:

```bash
sed -n '1,280p' docs/STATUS.md
```

Expected: STATUS is precise and factual while using Loomen verbs as organizing headings.

---

### Task 4: Rewrite ROADMAP Around Product Direction

**Files:**
- Modify: `docs/ROADMAP.md`

- [ ] **Step 1: Replace generic roadmap categories**

Start with:

```markdown
# Roadmap

Loomen should grow as a code-evolution instrument: local-first, evidence-rich, visually calm, and operationally precise. The roadmap is organized around making each Loomen verb more trustworthy.
```

- [ ] **Step 2: Organize roadmap by verbs**

Create sections:

```markdown
## Weave Better Paths
## Ray More Evidence
## Beam Clearer Sessions
## Pulse Reliable Validation
## Fuse With Confidence
## Sever Without Regret
## Product Atmosphere
## Architecture Health
## Distribution
```

- [ ] **Step 3: Keep roadmap concrete**

Include concrete items such as dependency checks, sidecar restart, interactive questions, structured session events, sibling workspace comparison, disk usage, unresolved comments, merge readiness, conflict previews, module splitting, schema migration tests, signing/notarization.

- [ ] **Step 4: Review ROADMAP manually**

Run:

```bash
sed -n '1,300p' docs/ROADMAP.md
```

Expected: every roadmap item feels specific to Loomen and avoids generic SaaS/dashboard language.

---

### Task 5: Verify And Commit

**Files:**
- Modify: `README.md`
- Modify: `docs/SEMANTICS.md`
- Modify: `docs/STATUS.md`
- Modify: `docs/ROADMAP.md`
- Modify: `docs/superpowers/plans/2026-04-30-loomen-product-direction-docs.md`

- [ ] **Step 1: Check frontend bundle syntax**

Run:

```bash
node --check dist/main.js
```

Expected: no output and exit code 0.

- [ ] **Step 2: Run Rust/Tauri tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: all 11 tests pass.

- [ ] **Step 3: Inspect changed docs**

Run:

```bash
git diff -- README.md docs/SEMANTICS.md docs/STATUS.md docs/ROADMAP.md
```

Expected: the diff shows documentation only, with Loomen product language as first-class vocabulary and engineering terms preserved.

- [ ] **Step 4: Commit**

Run:

```bash
git add README.md docs/SEMANTICS.md docs/STATUS.md docs/ROADMAP.md docs/superpowers/plans/2026-04-30-loomen-product-direction-docs.md
git commit -m "Rewrite Loomen product direction docs"
```

Expected: one commit containing the documentation rewrite and this plan.

