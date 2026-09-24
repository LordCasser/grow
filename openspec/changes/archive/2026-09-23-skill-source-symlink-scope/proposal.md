# Change: Preserve skill source scope across aliases

## Why

Automatic discovery currently assigns one scope to every skill under a config root, including descendants reached through symlinks. This can let a link promote a lower-priority source into a higher-priority Repo or Local source. Home-root spelling aliases can also bypass the special User-root identity check.

## What Changes

- Resolve the User `.grow` root by canonical identity so home aliases remain User scope.
- Preserve the selected config root's existing scope for files physically contained by its resolved root, including an explicitly selected root symlink.
- For descendant symlinks that leave that physical root, classify the canonical target and cap it at the root's trust ceiling: `Local < Repo < User`; a target may lower precedence but cannot gain higher precedence through an alias.
- Apply per-file source classification to discovered skills from automatic and explicit config roots.

## Capabilities

### Modified Capabilities

- configuration-rules: skill source scope across root and descendant aliases.

## Impact

Skill discovery precedence only. No filesystem writes or traversal policy changes.
