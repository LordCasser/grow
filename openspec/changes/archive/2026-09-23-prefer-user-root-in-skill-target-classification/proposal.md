# Change: Prefer User roots when classifying linked skill targets

## Why

Resolved skill targets are currently checked against the cwd `.grow` alias before the User roots. When that Local alias points at the User root, a Repo skill link into the User root is misclassified as Local and then capped to Repo, promoting User content.

## What Changes

- Classify canonical targets inside User roots as User before checking aliased Local or Repo roots.
- Add a regression where a Repo link reaches a User-root skill while cwd `.grow` aliases the same User root.

This follows the previously archived scope rule: a selected root alias keeps its entry scope for files inside that root, while a descendant link leaving another source root cannot gain precedence from an overlapping alias.

## Capabilities

### Modified Capabilities

- configuration-rules: target source classification precedence.

## Impact

Corrects skill precedence for descendant symlinks; no traversal or path storage changes.
