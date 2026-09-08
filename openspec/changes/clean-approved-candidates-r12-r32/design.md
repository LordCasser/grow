# Scope and constraints
R12/R14/R16/R17/R18/R22/R31 require renewed reachability and preservation checks. R13/R15/R19/R20/R23/R24/R25/R26/R28/R29/R30/R32 are authorized removals. R21 and R27 are separate product changes. Each removal must be independently tested and committed; retain active shared paths and user data.

Detailed implementation decisions and delta scenarios must be completed before corresponding code changes.

## R12 reviewed boundary
Whole Pager search found AgentSession.restore_degree only declared, initialized and assigned by load/fork; no runtime readers. Remove cache, internal TaskResult fields/arguments and cache-only assertions. Preserve both restore wire parsers (including unknown-value tests), shared types, restore execution and summary feedback. Regress session dispatch and parser groups before committing.

## R13 reviewed boundary
format_key_code_raw is cfg(test), referenced only by format_key_code_raw_shows_punctuation. Remove those two items only. Remaining input_log tests exercise the production sanitizer, ring capacity and private dump writing; run them before the separate R13 commit. No behavioral delta is needed for this test-only removal.

## R14 reviewed boundary
ActionId::DumpInputLog appears only in the enum and two no-op mapping arms; no ActionDef or construction exists. Remove these three locations. Preserve Action::DumpInputLog, Esc-d input translation, dispatch_dump_input_log and input diagnostics. Regress action registry, Esc-d guard and actual dump target selection.

## R15 reviewed boundary
Remove pager-render/gboom and render/gboom_overlay, the hidden slash command, AgentView game state and exclusive input/render handlers, action/router dispatch, simulation deadline branch and extra game keyboard layer. Preserve general animation/UI-state clocks, Kitty media protocol, mouse/focus handling and ordinary terminal restoration. Remove game-only assertions within mixed tests while retaining other modal/admission assertions. Compile both render and Pager consumers, run command registry, event-loop/clock and active media/input regressions.

## R16 reviewed boundary
Clipboard extension_for_class is cfg(test) and only macos_helpers::extension_mapping calls it. Remove helper and exclusive test module. Production pasteboard type selection, MIME mapping and file handling remain unchanged. Regress actual native type selection and MIME mapping; no behavioral delta for this test-only removal.

## R17 reviewed boundary
ManagedTextInspection.original_text is read only by one test assertion. Production consumers use unmanaged_text/requested_items/managed_block, while SourceState.bytes remains the authoritative transaction snapshot. Remove the duplicate field, accessor, allocation and exclusive test read/assertion; retain the rest of the inspection/apply test and transaction behavior. This is an internal representation/API cleanup without a changed persistence contract. Regress managed_text including backup/rollback and unmanaged preservation.

## R18 reviewed boundary
The old load/build/orphan chain has only self-tests and two prompt-widget test consumers. Its collapse_strip_seam and make_real_image fixture are exclusive to that chain. Remove them and exclusive tests; retain PastedImage persistence round-trip by asserting the persisted file, and prompt stash/deleted-chip ownership assertions directly against drained images. Actual append_prompt_images already verifies memory/disk equivalence and bounded admission; run those plus retained prompt_images/prompt-widget tests. Do not remove active placeholder loaders, image numbering, caches, viewer or persistence.

## R19 reviewed boundary
Dedicated key/helper reads exist only in tests; writes are the shared placeholder recovery and live append_prompt_images. Remove that protocol key, helpers, writes and exclusive metadata assertions. Preserve numbered text anchors and PastedImage.display_number; preserve all generic _meta passthrough. Current client-surfaces requirement on image numbering is fulfilled by the preserved textual/display identities, not the unused metadata interpreter (which does not exist). Regress recovery, actual send and generic metadata passthrough.

## R20 reviewed boundary
open_from_path is a separate synchronous constructor with exactly three direct tests. Preserve their PNG/JPEG dimensions, byte and display-number assertions by using open_from_path_deferred + finish_loading (which delegates to the actual load_image_data/apply_loaded path); preserve failed-path coverage there too. Remove only synchronous constructor and its obsolete preference comment. The actual background admission is already wired in AppView::prepare_agent_image_load; no additional viewer redesign is required.

## R22 reviewed boundary
/debug scroll already dispatches exactly ToggleScrollDebugHud. Remove the alias module/registry/reserved-name classification and exclusive assertions. Keep debug scroll assertion, actual HUD and environment enablement; update live HUD hint and comments to the supported command. Regress slash registry/debug and HUD rendering.

## R23 reviewed boundary
ActivePaneSnapshot::Other has no constructors or match consumers; record_input maps every actual pane explicitly. Remove only this unused diagnostic enum variant and regress input_log. No runtime behavior change or new contract.

## R24 reviewed boundary
Announcement.persistent participates only in serde and default assignment; no client filter/display consumer exists. Remove field/default, preserve dismissible, expires_at, hidden-ID persistence and update payloads. Regress announcement library and actual client announcement tests.

## R25 reviewed boundary
Diagnostics id module has no internal callers; mid is declared only by diagnostics and UUID v5 is used there only for the device hash. Remove module/export/mid dependency and the exclusive diagnostics v5 feature request, preserve UUID v4/v7/session identity and user files. Refresh the lockfile without upgrading dependencies; verify remaining package entries unchanged and run isolated diagnostics tests plus locked metadata resolution.

## R26 reviewed boundary
snapshot_session_log has no callers, while snapshot_log and writer/trimming are active and retain tests. Remove only the unused filter function and its doc comment; run the retained unified_log tests (the crate installs a pre-main temporary-log redirect).

## R28/R29 reviewed boundary
read_optional_json_sync, summary_lock_file and workflows_dir are private and uncalled across Rust sources. Remove R28 reader in its own commit, then R29 path helpers in another. Retain actual directory-capability reads, Timeline/control restoration, writer lease and rewind_points_file (test consumers). Run relevant JSONL storage regressions for each deletion; never delete user files or directories.

## R30/R31 reviewed boundary
R30 standalone rewind_files and its three response/conflict types have no callers; remove only that region, preserving tracker/ToolContext, snapshots and Shell transaction rewind. R31 removes only merge_and_remove_from/max_prompt_index. Migrate historical merge assertions to get_rewind_points + pure merge_rewind_points_from; retain lazy-load failure/retry checks and max-index assertion derived from complete points. Do not remove truncate_from, replace_rewind_points or get_rewind_points. Run workspace file_state tests after each independent deletion.

## R32 reviewed boundary
Only CountingCallback implements the optional attribution hook; all production config paths use None or forward it. Remove callback trait/alias/consumer enum, config/client fields, six optional calls and propagation. Keep SentRequest.sent_bearer capture, auth_rejected/SentCredential, auth_info method classification and auth retry budget. Keep bearer_tail_fragment/constant because active auth capture still consumes them; do not fold a credential-presence redesign into removal. Convert useful sent-fragment assertions in the callback test to direct post() assertions; remove callback-only/no-op test. Regress sampler auth/request handling and compile Shell/workflow forwarding consumers.
