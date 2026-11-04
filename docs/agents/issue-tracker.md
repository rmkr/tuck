# GitHub issue tracker

Specs and tickets live in GitHub Issues for `rmkr/tuck`. Use the `gh` CLI from this repository.

## Issue operations

- Create issues with `gh issue create --title "..." --body-file <file>`.
- Read issues with `gh issue view <number> --json number,title,body,labels,comments`.
- List issues with `gh issue list` and the relevant label and state filters.
- Write comments with `gh issue comment <number> --body-file <file>`.
- Add or remove labels with `gh issue edit <number> --add-label "..."` or `--remove-label "..."`.
- Close completed issues with `gh issue close <number>`.

Use a temporary UTF-8 file for multiline issue bodies and comments.

## Specs and dependencies

When a skill publishes a spec, create a GitHub issue. When it fetches a ticket, read the corresponding issue.

Use GitHub sub-issues and native issue dependencies when available. Dependency API calls require the issue's numeric database ID from `gh api repos/rmkr/tuck/issues/<number> --jq .id`.

If sub-issues are unavailable, put `Part of #<parent>` at the top of the child and add it to a task list in the parent. If native dependencies are unavailable, record `Blocked by: #<number>` at the top of the blocked issue. A ticket is ready when its blocking issues are closed.

## Pull requests as a triage surface

**PRs as a request surface: no.**
