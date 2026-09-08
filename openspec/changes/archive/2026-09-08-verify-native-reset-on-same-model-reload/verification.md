# Verification

New shell regression same_model_catalog_reload_discards_signed_native_history passed 1/1 in 0.04s. Its three cases cover unchanged ModelId with endpoint, query-route or wire-model changes. Each pre-reload request was asserted to carry native continuation; each post-reload request had none, retained portable answer text and exposed the new config. This is a passing regression for existing behavior, not a red/green bug fix.

No production code changed. No live LLM or real user config request occurred. The test uses a temporary actor and directly admitted visible/native response fixtures; it does not establish remote credential-rotation or invisible upstream deployment behavior. Existing linker compact-unwind warning was nonfatal.

Scoped rustfmt and git diff --check passed; strict pre-archive all 17/17 passed. No cargo/rustc process remained before cargo clean. Removed 7,372 files / 2.7 GiB; free disk 56 GiB.
Post-archive strict all 16/16 and archives 264/264 passed.
