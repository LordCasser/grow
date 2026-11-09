# Verification

## Source identity and documentation recovery

- Baseline: `8fbc52185ce30e38d97745dd6597242da1135c31` on `LordCasser/grow/main`.
- Validated publication head: `a3768b3b077b59695314d0fe58e7782181bc9fab`; source tree: `bf0f734ad420b8fbe9d30a7d053ca9678ffeee22`. Its tested PR merge commit `5655f951ae4b295472d3b248c11cb4d51c8b1691` has the same tree.
- This document was reconstructed on 2026-11-09 from the published source and recorded verification outcomes after the temporary checkout became unavailable. It is a new documentation revision, not a claim of byte identity with the earlier unpublished closure record. Publication of the recovered documentation requires fresh checks on its own final head.
- The recorded `rg --hostname-bin` and MCP cancellation fixes were already in the baseline. Messages signature concatenation remains deliberately unadopted pending supported-route evidence. This change implements only the accepted catalog/provider name separation.

## Runtime evidence

The following completed CI runs were rechecked on 2026-11-09 and report success for the exact head above:

- [Core regression](https://github.com/LordCasser/grow/actions/runs/42504739402).
- [OpenSpec](https://github.com/LordCasser/grow/actions/runs/42504739385).
- [Local agent coordination](https://github.com/LordCasser/grow/actions/runs/42504739386), including Ubuntu, macOS and Windows jobs.

The [Core job](https://github.com/LordCasser/grow/actions/runs/42504739402/job/128804818308) completed the full Shell library suite with **4,794 passed, 0 failed, 13 ignored**. Its MCP catalog lifecycle selection contains **17 passed, 0 failed, 0 ignored**. The ignored tests in the full suite are not counted as executed.

The actual Shell tests cover:

- Real MCP initialization/listing and registration, then snapshot/search/schema retrieval and `use_tool` dispatch of the unchanged raw tool name and arguments.
- Qualified names at 64, 65 and 256 characters, 257 rejection, and distinct long names with the same first 64 characters.
- App-only visibility, disabled tools and live-parent eligibility revocation.
- Actual SessionActor `execute_tool_calls` authorization through the real permission manager, using distinct full-length names. The allow and deny cases assert `policy_allow` and `policy_deny`, respectively, and rejected calls do not increment the server call count. A generic prompt-policy rejection is not accepted as evidence of exact deny-rule matching.
- The public Agent sampling projection alongside the complete ToolBridge catalog.
- The public asynchronous descriptor materialization entry, longest accepted raw names, repeated writes, ordinary flat paths, reserved-directory separation and cleanup.

## Focused local checks and old-code controls

- Official Rust **1.93.1**, Linux x86_64: `cargo test --locked -p agent -p mcp --lib -- --test-threads=4` completed with Agent **504 passed** and MCP **200 passed**.
- Removing only the Agent projection filter makes `agent::tests::sampling_definitions_preserve_provider_names_and_complete_bridge_catalog` fail with eight declarations instead of the expected three. Restoring the filter passes. This drives the public Agent method and compares the complete bridge catalog, schemas and ordering.
- MCP unit controls cover length and syntax boundaries, longest server/raw segments, app-only metadata, descriptor path components and fallback namespace separation.
- A real filesystem probe accepts raw length 250 plus `.json` and rejects raw length 253 plus `.json` with `ENAMETOOLONG`. The latter is still a legal 256-character `s__<raw>` catalog identity.
- An exact-source descriptor control compiled all eight MCP source files from the baseline and the validated source separately against the same compatible cached dependencies. It used the actual asynchronous public entry and the committed Shell descriptor regression body with only direct-crate imports adapted. The baseline produced one of the three required descriptors and failed; the changed source passed, including distinct longest raw names, schema preservation, repeated publication, ordinary reserved-name handling and cleanup.
- The same public validator control accepts lengths `[64, 65, 256, 257]` as `[true, false, false, false]` on the baseline and `[true, true, true, false]` on the changed source. This focused exact-source comparison is not a full baseline workspace test run.
- The descriptor source SHA-256 values were `333bacbc4ff132c7a0cfb3494e0e7b34f4d25e8ed23f9df59d64804fd88e048e` for baseline `servers.rs` and `99b2c715c011fae314aff1ddf824cfbe01760692a43f20a6b6663249923f85f3` for the changed file.
- Integrated Shell Clippy completed with exit 0. Non-fatal warnings remain; this is typechecking/lint evidence, not runtime coverage. Formatting and whitespace checks passed.
- Independent code review found no remaining concrete production or fixture defect after the exact permission decision-reason assertions were added.

## Limits and closure

Full local Shell test-binary builds were terminated before test execution, including a single-job attempt using Rust's bundled lld. Those attempts are not test passes, and unavailable kernel evidence does not establish a specific resource-failure cause. The completed full remote Shell suite supplies the independent runtime result.

The local transport fixtures use the production ACP reverse bridge with an in-process MCP server. They do not establish behavior against every remote MCP provider or every native filesystem. The cross-platform coordination workflow is not a substitute for a Windows-specific long-descriptor test.

The descriptor helper has no in-repository runtime caller at the baseline but remains public. Only names that previously exceeded a flat filesystem component use the nested layout. Existing flat paths, original identities, visibility and authorization ownership remain unchanged. Best-effort handling of arbitrary upstream names outside the accepted catalog contract and pre-existing sanitizer collisions are not expanded or repaired here.

The deferred ACP gateway experiment and model-pool implementation are outside this change. Later publication or integration heads must be verified separately; success of the source head linked above is not a claim that a newer head has already passed.
