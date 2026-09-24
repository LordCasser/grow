## Evidence

- Field-by-field parser review found `coerce_to_string` accepted YAML strings, booleans and numbers. `name` feeds slash identity; `description` and `when-to-use` feed skill-selection metadata; license, compatibility and argument hint are forwarded as display fields. The parser now reads only YAML strings, keeping the established directory-name/body-description fallbacks and omitting invalid optional text.
- `coerce_tool_list` already rejected a wrong top-level type but filtered non-string sequence members, yielding a partial declaration. It now omits a mixed list atomically. Prior archived audit `2026-09-07-audit-skill-allowed-tools-consumers` found no authorization consumer: this remains display metadata only.
- `parse_metadata` already restricts projected entries to string keys and string values. `paths` and the two invocation switches already reject invalid types. `model` and `effort` have no direct skill-parser consumer. These paths were not changed.

## Validation

- `cargo test --locked --offline -p tools --lib implementations::skills::discovery::tests --quiet`: 43 passed, 0 failed. Includes new regressions for non-string name/description/trigger/display fields and mixed `allowed-tools`, plus existing string/list and fallback coverage.
- `rustfmt --edition 2024 --check crates/codegen/tools/src/implementations/skills/discovery.rs`: passed.
- `git diff --check` over changed source, guide and change files: passed.
- `openspec validate --all --strict --no-interactive`: 20 passed, 0 failed.
- After archive, `openspec validate --all --strict --no-interactive`: 19 passed, 0 failed; `openspec validate --archived --no-interactive`: 381 passed, 0 failed.

## Limits

`allowed-tools` still does not restrict Grow tool access. The change corrects the parsed and displayed declaration only. Arbitrary metadata continues to retain valid string entries while skipping non-string entries; no runtime or persistence behavior changed.
