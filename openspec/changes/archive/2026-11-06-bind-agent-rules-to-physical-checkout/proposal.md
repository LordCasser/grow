# Bind Agent rules to the physical checkout

## Why

Rule discovery compares a canonical Git checkout root with lexical cwd ancestors. A symlink outside the checkout that points to an internal subdirectory misses the repository root rules and can add the alias's external ancestors (AC-04).

## What Changes

Build this rule owner's cwd-to-checkout chain from one canonical cwd identity and the existing physical checkout discovery. Use physical paths for project rule attribution, retain root-to-cwd order and explicitly configured user roots, and test aliases into the root and internal subdirectories plus linked worktrees. Other repository walkers and trust policy remain separate.
