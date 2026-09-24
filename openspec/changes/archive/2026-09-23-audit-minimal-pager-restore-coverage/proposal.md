# Audit minimal external pager restore coverage

## Why

The default transcript pager test sets `PAGER=cat`, so it does not exercise an interactive child pager or terminal restoration after that child exits. The repository has a real `less` PTY regression, but it remains ignored. A default-test enablement attempt was interrupted before producing a test result because the build consumed substantial shared disk space.

## What Changes

- Record the exact remaining coverage gap and the existing ignored `less` regression.
- Keep the test ignored and retain the backlog item until it can be verified with a bounded build strategy.

## Impact

Audit and developer guidance only. No runtime behavior or archived contract changes; `skip_specs: true` is appropriate.
