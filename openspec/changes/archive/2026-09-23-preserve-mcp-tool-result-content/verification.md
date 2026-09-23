# Verification record

## Baseline and bounds

- rmcp 2.2 `CallToolResult` carries ordered content, `structured_content`, `is_error`, and `_meta`; content variants are text, image, audio, embedded resource, and resource link.
- Main dispatch is `McpErasedTool::run` → inner `ToolDyn` typed stream → `FinalizedToolset::call_raw` JSON conversion → `InnerDispatchForToolset` second JSON conversion → `use_tool` truncation → outer `FinalizedToolset::finalize_output` third JSON conversion → `SessionActor::handle_bridge_tool_success` → Timeline tool result and image follow-up. All three conversions retain images through `TypedToolOutput.model_output` Image blocks and restore them to `MCPOutput`; ACP `raw_output_json` serializes only the redacted `ToolOutput`, so runtime image bytes are `#[serde(skip)]` there.
- Existing data-URI image bounds are 10 MiB encoded payload and five images. Typed MCP images use the same admission values before Shell normalization. Direct native JSON response budget is 16 MiB; a counting writer checks it without allocating a second full result buffer.
- Each MCP text/resource-text block extracts inline data-URI images before advancing to the next block, sharing the five-image budget with typed image blocks. Shell skips its generic whole-prompt extractor for MCP output so a later text URI cannot be reordered before an earlier typed image.

## Executed checks

| Command | Exit | Result |
| --- | ---: | --- |
| `openspec validate preserve-mcp-tool-result-content --strict --no-interactive` | 0 | Change schema and scenarios valid. |
| `git diff --check -- crates/codegen/mcp/src/servers.rs crates/codegen/tools/src/types/output.rs crates/codegen/tools/src/util/base64_images.rs crates/codegen/tools/src/util/mcp_truncate.rs crates/codegen/shell/src/extensions/mcp.rs crates/codegen/shell/src/session/actor/tool/result.rs docs/development.md` | 0 | No whitespace errors in touched source/docs. |
| `cargo test --locked -p mcp --lib mcp_projection_ -- --nocapture` | 101 | Cargo refused because the lock file needs an update. No unlocked retry was attempted. |
| `cargo test --locked -p mcp --lib mcp_projection_ -- --nocapture` | 0 | Initial structured/nontext/dedup/invalid-image projection tests passed 3/3. Rerun after the later interleaved-inline-image change. |
| `cargo test --locked -p tools --lib mcp_typed_image_survives_text_truncation_without_raw_json_leak -- --nocapture` | 0 | Long text was truncated/dumped; runtime image remained; serialized raw JSON omitted base64. Rerun after later three-boundary changes. |
| `cargo test --locked -p shell --lib test_mcp_call_response_serialization -- --nocapture` | 0 | Native mixed content, `structuredContent`, `_meta` direct wire serialization passed 1/1. Only a linker compact-unwind warning occurred. |
| `cargo test --locked -p mcp --lib mcp_projection_ -- --nocapture` | 0 | Latest block projection suite passed 4/4, including interleaved text data URI / typed image / text-resource data URI ordering. |
| `cargo test --locked -p tools --lib erased_local_registry_tool_emits_image_in_typed_model_output -- --nocapture` | 0 | Real `LocalRegistry::ErasedTool::execute` populated `TypedToolOutput.model_output` with an Image; JSON `value` omitted its base64. |
| `cargo test --locked -p tools --lib inner_mcp_dispatch_preserves_runtime_image_across_json_value -- --nocapture` | 0 | `InnerDispatchForToolset`/`use_tool` value conversion preserved the image. |
| `cargo test --locked -p tools --lib mcp_runtime_image_crosses_typed_dispatch_without_raw_output_leak -- --nocapture` | 0 | MCP image survived typed JSON restore while serialized raw output remained redacted. |
| `cargo test --locked -p shell --lib use_tool_image_reaches_shell_timeline_followup_without_raw_base64 -- --nocapture` | 0 | Actual `use_tool` through three value conversions and Shell result: long text was truncated, runtime image survived, Timeline tool text omitted base64, image follow-up was committed; `Timeline::from_events` replay and next `build_request` retained one image. Reran after adding truncation/replay assertions; passed 1/1. Linker compact-unwind warning only. |
| `cargo test --locked -p shell --lib direct_mcp_result_over_limit_is_rejected_without_partial_json -- --nocapture` | 0 | More than 16 MiB direct result rejected without partial success JSON; passed 1/1. |
| `cargo test --locked -p pager --lib mcp_image_result_renders_redacted_use_tool_text -- --nocapture` | 0 | Production `tool_call_to_block` + Expanded `BlockContent::output` rendered the MCP image placeholder and did not render raw base64; passed 1/1. This checks actual TUI block text, not terminal graphics protocol. Linker compact-unwind warning only. |
| `cargo test --locked -p shell --lib mcp_result_tests -- --nocapture` | 0 | Final two-test run passed 2/2 after switching the fixture to a valid 128×128 PNG admitted by `admit_typed_image`. Long MCP text truncated while the image normalized and reached follow-up; real `JsonlStorageAdapter` durable append + reopen by id and `project_portable_history` each retained one ordered image group. Non-MCP text tool still extracted a valid inline data URI through the generic Shell path. Linker compact-unwind warning only. |
| `openspec validate --all --strict --no-interactive` | 0 | Latest run after sibling changes were archived: 17 current specs/changes passed, 0 failed; this change's archive and archived-spec revalidation remain pending. |

## Known scope limits

- `Cargo.lock` was reconciled by the parent task; all focused MCP/tools/Shell/Pager tests above pass. The Shell regression exercises real image admission and normalization, in-memory Timeline replay, next model `build_request`, canonical JSONL durable storage reopen, and provider-neutral portable history projection. Image rejection/count-budget and interleaved order are separately covered at the MCP projection boundary.
- Provider-specific Chat Completions/Responses/Messages wire assembly, a physical PTY screenshot, and terminal graphics protocol are not newly exercised; neither is required by this change's model-neutral image attachment contract. The TUI check covers the actual UseTool block renderer and base64 redaction.

## Archive integration

- `openspec validate --all --strict --no-interactive`: exit 0, 17/17 before archive.
- `openspec archive preserve-mcp-tool-result-content --yes`: exit 0; added one `extension-runtime` and one `session-timeline` requirement under `2026-09-23-preserve-mcp-tool-result-content`.
- Archived task 3.4 was checked only after archive completed; full current validation passed 16/16 and archived validation passed 369/369 (both exit 0).
