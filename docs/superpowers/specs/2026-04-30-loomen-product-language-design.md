# Loomen Product Language Design

## Purpose

Loomen should fully embrace the remote workbench direction without becoming a plain engineering dashboard. The product language must preserve the original Loomen vocabulary while making the current Tauri workbench understandable, debuggable, and credible.

The goal is not to map old CLI commands onto new UI controls one by one. The goal is to recover the first principles underneath those words and let them guide the docs, roadmap, and eventually the visible and invisible corners of the product.

## First Principles

AI coding changes software development from a single line of edits into multiple candidate futures. A serious tool for that world must give each future:

- a place to exist without corrupting the others
- a way to be observed while it evolves
- a way to receive repeatable validation signals
- a way to be compared against sibling paths and the base line
- a path back into the main codebase when it proves valuable
- a graceful end when it is no longer worth carrying

Loomen's original verbs are the right primitives for this model:

- **Weave**: create a candidate path.
- **Ray**: make the path's state visible.
- **Beam**: observe the live flow inside a path.
- **Pulse**: send validation or action through one or more paths.
- **Fuse**: bring a proven path back home.
- **Sever**: stop carrying a path.

These words are product primitives, not decorative labels.

## User Experience Principle

Loomen vocabulary should be visible throughout the product, but never at the cost of orientation. The rule is:

> poetic action, plain object.

Good UI and docs phrases:

- Weave a workspace
- Ray this path
- Beam session output
- Pulse tests
- Fuse through review
- Sever archived work

The Loomen verb carries the identity. The plain object explains the effect.

Engineering terms remain valid and necessary: repository, workspace, worktree, branch, session, checkpoint, diff, PR, archive. They are the physical body of the Loomen actions. The docs should not hide these terms or rename every implementation object into metaphor.

## Brand Tone

Loomen should feel precise, quiet, and luminous. It should not feel like a fantasy universe simulator or a gimmicky agent launcher.

Preferred tone:

- local-first
- calm
- exact
- slightly poetic
- oriented around evidence and review

Avoid:

- overusing "parallel universe" as the main explanation
- implying automatic semantic merging
- describing agents as magic workers
- turning every noun into a metaphor
- making git behavior sound hidden or abstract

The core brand sentence can be:

> Loomen gives every possible future of your code a place to live, a way to be seen, and a path back home.

## Documentation Design

### README

The README should open from first principles:

- AI coding creates multiple candidate futures.
- Loomen makes those futures local, isolated, observable, comparable, and deliverable.
- The six Loomen verbs are the product model.

The feature list should then connect each verb to current implementation:

- Weave: repositories, workspaces, git worktrees, branches.
- Ray: files, diffs, search, PR/check state, context usage, terminal evidence.
- Beam: Claude/Codex sessions, streaming output, tool approvals.
- Pulse: setup scripts, run scripts, PTY terminals, future multi-workspace broadcast.
- Fuse: checkpoints, diff review, comments, PRs, checks, merge readiness.
- Sever: archive, cleanup, branch/worktree deletion preview.

### SEMANTICS

`docs/SEMANTICS.md` should become the product language charter. It should explain:

- the first-principles model
- the six verbs
- the plain engineering body behind each verb
- the rule that Loomen words are first-class but not allowed to obscure git reality
- local-first and source-available boundaries

### STATUS

`docs/STATUS.md` should remain factual. It should not read like branding copy. Organize implemented capability under Loomen verbs, but keep exact implementation details in each bullet.

Example:

- **Weave**: real `git worktree add`, branch naming, repository import.
- **Beam**: Unix-socket sidecar, Claude/Codex adapters, streaming events.

### ROADMAP

`docs/ROADMAP.md` should express future work through the same verbs, with an explicit architecture health section. The roadmap should avoid generic categories that could describe any app.

It should answer: what would make Weave, Ray, Beam, Pulse, Fuse, and Sever more trustworthy?

## Scope

This design only covers documentation and product language. It does not yet rename UI controls, database fields, Rust functions, TypeScript state, or command IDs.

Future implementation may gradually carry this language into visible UI labels, command palette actions, settings descriptions, event names, and tests. That should happen carefully, with engineering names preserved where they make the system easier to maintain.

## Success Criteria

- A new reader understands Loomen without knowing the old CLI.
- A returning reader recognizes the original Loomen vocabulary.
- The docs no longer feel like a translation table from old commands to new features.
- The roadmap feels specific to Loomen rather than a generic desktop app checklist.
- The product sounds poetic but remains operationally precise.

