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
