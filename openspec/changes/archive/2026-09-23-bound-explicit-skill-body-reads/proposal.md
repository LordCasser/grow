## Why

Skill body loaders currently read arbitrary file sizes into memory before prompt assembly can apply its inline budget. A single very large `SKILL.md` can therefore cause unbounded temporary allocation during slash invocation, agent preloading, or workflow capture.

## What Changes

- Bound file-backed Skill body reads to 1 MiB plus one probe byte.
- Return an explicit over-limit error and reject the whole body instead of injecting or freezing a truncated prefix.
- Preserve `load_skill_content`'s existing in-memory body snapshots and synthetic-path behavior; workflow freezing continues to read the current file.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `configuration-rules`: require bounded file-backed explicit Skill body loading while preserving complete-body and snapshot semantics.

## Impact

- `crates/codegen/tools/src/implementations/skills/skill.rs` will share a bounded file-read path across ordinary body loading and workflow body freezing.
- Focused tests will cover the exact limit and rejection through both loaders.
- No caller signatures or dependencies change; existing callers already propagate or omit skills on loader errors.
