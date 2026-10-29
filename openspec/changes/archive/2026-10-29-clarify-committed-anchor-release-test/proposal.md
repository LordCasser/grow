## Why

`insert_block_before_rejects_an_already_committed_anchor` expects a panic from `debug_assert!`, which is compiled out in release. Its unconditional `should_panic` therefore creates a release-only failure unrelated to the release behavior being exercised.

## What Changes

- Compile the panic expectation only with debug assertions enabled.
- Keep the production precondition and all other insertion, selection, and commit-frontier tests unchanged.

## Scope

This is test configuration maintenance only. The insertion contract and runtime behavior do not change; `.openspec.yaml` uses `skip_specs: true` and adds no delta spec.
