# Audit minimal external pager restore coverage

## Why

The default transcript pager case uses `PAGER=cat`, which cannot prove that an interactive pager child exits and restores the minimal terminal UI. A `less` PTY regression exists, but it remains ignored and a targeted run did not reach the pager path.

## What Changes

- Record the existing real-pager test, its current ignored status, and the bounded run evidence.
- Preserve the ignored test and the backlog coverage item until a passing run exercises the `less` child.

## Impact

Audit and developer guidance only. No runtime, test-selection, or archived contract change; `skip_specs: true` is appropriate.
