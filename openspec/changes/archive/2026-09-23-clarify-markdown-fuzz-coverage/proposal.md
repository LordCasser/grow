## Why

The Markdown fuzz README and manifest comment claim that `render_all` exercises Syntect combinations, while the target always passes `None` and covers only four renderer paths. Correcting those claims prevents readers from treating unexecuted paths as fuzzed.

## What Changes

- Describe the actual pretty/non-pretty full and streaming render calls, UTF-8-safe chunk rotation, and the target's crash/panic-only oracle.
- Correct the manifest comment to match the README and target.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. This is a documentation-only correction; no runtime behavior or contract changes.

## Impact

`crates/codegen/markdown/fuzz/README.md`, the adjacent `Cargo.toml` target comment, and this OpenSpec change. No code, dependency, or runtime behavior changes.
