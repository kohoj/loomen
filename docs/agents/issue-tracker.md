# Issue Tracker: GitHub

Issues and PRDs for this repo live in GitHub Issues for `kohoj/loomen`. Use the `gh` CLI for all operations.

Prefer passing `--repo kohoj/loomen` explicitly. This keeps skills stable even if the local `gh` installation cannot infer the repository from the SSH remote.

## Conventions

- **Create an issue**: `gh issue create --repo kohoj/loomen --title "..." --body "..."`. Use a heredoc for multi-line bodies.
- **Read an issue**: `gh issue view <number> --repo kohoj/loomen --comments`, filtering comments by `jq` and also fetching labels.
- **List issues**: `gh issue list --repo kohoj/loomen --state open --json number,title,body,labels,comments --jq '[.[] | {number, title, body, labels: [.labels[].name], comments: [.comments[].body]}]'` with appropriate `--label` and `--state` filters.
- **Comment on an issue**: `gh issue comment <number> --repo kohoj/loomen --body "..."`
- **Apply / remove labels**: `gh issue edit <number> --repo kohoj/loomen --add-label "..."` / `--remove-label "..."`
- **Close**: `gh issue close <number> --repo kohoj/loomen --comment "..."`

If a command still needs the repository name, use `kohoj/loomen`.

## When a skill says "publish to the issue tracker"

Create a GitHub issue.

## When a skill says "fetch the relevant ticket"

Run `gh issue view <number> --repo kohoj/loomen --comments`.
