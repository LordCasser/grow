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
