# Verification

All Cargo commands used `--locked`, `GROW_HOME=/tmp/grow-t1-home`, and
`RUST_MIN_STACK=16777216`.

The final targeted differential run was:

```text
cargo test --locked -p fsnotify --lib bounded_per_dir_selection_matches -- --test-threads=1
```

Result: 2 passed. The tests compare candidate sequences against a typed
stable-sort/truncate semantic oracle across mixed, equal, cyclic-up, and
cyclic-down depth patterns and N/B boundaries. The oracle precomputes depth
once; it preserves old selection order but is not a verbatim measurement of
the previous production comparator. A reversed equal-depth input checks that
the selected order follows input ordinal even when lexical path order is
opposite, at B=8 and B=49,152. A second test compares the oracle and final
selector against an actual 12,000-directory tree at B=8 and the default
budget.

The final full package run was:

```text
cargo test --locked -p fsnotify
```

Result: library tests 119 passed, 15 ignored; integration tests 2 passed, 6
ignored; doc tests 0. It ran after removing the temporary ignored timing probe
and helpers from `watcher.rs`.

The first heap-only timing probe and final budget-aware probe each used two
warmups and seven raw samples. Their output is preserved in
[measurement-after.txt](measurement-after.txt) and
[measurement-budget-aware.txt](measurement-budget-aware.txt); source is in
[probe-after.rs](probe-after.rs) and [probe-budget-aware.rs](probe-budget-aware.rs).
The earlier old-selector baseline log is in [measurement-before.txt](measurement-before.txt).
The source-only pre-change snapshot path is recorded at
`/tmp/grow-t1-source-before-watcher-path.txt`.

The after-probe raw label `pure` is historical. `timing_samples` measures the
full run closure: owned-input samples include `candidates.clone`, selection,
and dropping the returned paths. Fixture construction and equality checks are
outside the timed closure. The paired old implementation is the typed
precomputed-depth stable-sort reference described in [measurement.md](measurement.md),
not a verbatim replay of the old `components().count()` sort comparator. The
314.60 ms before-production filesystem result is an independent run, not the
paired old reference value.

Formatting and whitespace checks:

```text
rustfmt --edition 2024 crates/codegen/fsnotify/src/watcher.rs
git diff --check -- crates/codegen/fsnotify/src/watcher.rs docs/development.md openspec/changes/bound-initial-per-directory-watch-selection
```

Strict OpenSpec validation passed:

```text
openspec validate --all --strict --no-interactive
```

Result: 18 passed, 0 failed. Independent review and archive remain with the
parent integration pass.
