# Verification

- `cargo test --locked -p shell --lib model_api_backend_overrides_provider_default_in_sampling_config`: 1 passed. The test checks provider inheritance, model overrides to Messages and Chat Completions, the absent-at-both-levels default, and the model-specific endpoint URL in the resulting sampling configuration.
- `rustfmt --edition 2024 --check crates/codegen/shell/src/agent/provider_catalog.rs`: passed.
- `git diff --check` for the changed Rust and documentation files: passed.
- `openspec validate --all --strict --no-interactive`: 15 items passed, 0 failed before archive.
- `cargo fmt --all -- --check` reports formatting differences in `pager/src/lib.rs` and `workspace/src/file_system/fuzzy.rs`, neither modified by this change. The changed Rust file passes its scoped formatting check.

The resolver, sampler configuration transfer, and sampler backend dispatch were inspected before the test was added. The change formalizes existing behavior and adds no runtime branch.
