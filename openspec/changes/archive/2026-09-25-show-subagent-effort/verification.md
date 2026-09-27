# Verification

- `cargo check --locked -p pager -p shell -p pager-render` and `cargo check --locked -p pager -p shell --tests` passed.
- Focused Shell tests covered optional spawn-effort serialization and canonical spawn-fact projection.
- Focused Pager tests covered the Tasks row, shared model/effort formatter, spawn and later model-change synchronization, replay precedence, and first-open child replay synchronization.
- `rustfmt --edition 2024 --config skip_children=true --check` on changed Rust files, `git diff --check`, `openspec validate --all --strict --no-interactive`, and archived OpenSpec validation passed.

The tested scenarios include an old spawn without effort, an explicit child effort, a later child effort change, and replay of an older spawn after the change.
