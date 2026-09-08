## Verification
Read-only symbol/caller audit across repository source found no production caller of the old builder chain, while append_prompt_images has production calls in effects/mod.rs at normal send and snapshot send. Checked helper behavior directly, not merely its name. Producer search includes the real helper's shell re-exported metadata setter.

The dedicated imageDisplayNumber reader occurs only in tests; generic metadata preservation was explicitly reviewed in image_normalize.rs and input_inbox.rs. The audit does not claim no external ACP consumer. Image #0 is explicitly retained by an existing extraction test; no unsupported-number bug was inferred from its value alone.

Temporary deletion list now has R18/R19 with bounded deletion and preservation scopes. No Rust, Cargo, behavior, test assertion or main spec edited in this audit; no Cargo test run was necessary. No file or feature was deleted. skip_specs documents why no delta is supplied.
