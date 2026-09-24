## Why

Skill frontmatter currently converts YAML booleans and numbers into strings for `name`, `description`, `when-to-use`, `license`, `compatibility`, and `argument-hint`. A malformed `name: true` becomes the command identity `true`; malformed description and trigger values become model-visible text, and a non-string trigger can make a plugin skill appear eligible for listing. `allowed-tools` also drops invalid items from a YAML list and displays a partial declaration. This changes skill identity and discovery text without identifying the bad type.

The prior `allowed-tools` consumer audit established that the field is display metadata in Grow and does not restrict tool access. This change corrects its display parsing only; it does not add authorization behavior.

## What Changes

- Treat descriptive scalar fields as YAML strings only. A non-string `name` follows the existing directory-name fallback; non-string `description` follows the existing body-description fallback; invalid optional trigger and display fields are omitted.
- Retain the existing accepted `allowed-tools` forms (delimited string and YAML string list), but omit the entire field when a list contains a non-string item instead of displaying a partial list.
- Preserve invocation-switch defaults, `paths` gating, arbitrary metadata's existing string-entry filtering, tool permissions, and all valid string inputs.

## Capabilities

### Modified Capabilities
- `configuration-rules`: Skill metadata scalar and list type handling.

## Impact

- Parser: `crates/codegen/tools/src/implementations/skills/discovery.rs` and its tests.
- User guidance: `crates/codegen/pager/docs/user-guide/08-skills.md`.
- No wire format or persisted data changes. `allowed-tools` remains descriptive metadata only.
