# Verification

## Evidence

The second reported session, `01a15323-b21a-7403-906e-72048e8b7594`, has SuspendRequested at seq 5956 and CommandsSettled at 5957, with no completed checkpoint at inspection. Its process was present at the sampling time; a later read-only check no longer found that PID, so this record makes no claim about its subsequent exit cause. A one-second read-only native sample of pid 17931 showed the session thread waiting in kevent, not a sustained CPU-bound loop; the stripped binary does not identify the exact pending future, so this does not prove which physical owner remains outstanding. No keys, cancellation, resume or commands were sent to either user session.

Code inspection independently confirmed two UI defects: Quit input is swallowed for both Pending and Unconfirmed; minimal draw returns into its own hook before the normal exit painter, and the minimal renderer never painted this state. Normal Welcome also returned before the overlay, and the original wrapped error could displace recovery controls on narrow screens. These defects explain missing retry affordances without assuming that every reported freeze had one cause.

## Changes and coverage

Pending still coalesces and keeps its original deadline. Unconfirmed uses the existing two-press Quit confirmation and dispatches RetryExitSuspension; r/f remain explicit recovery actions and clear a partial Quit arm. Only the matching current exit_id can settle it. Normal Welcome/Agent/Dashboard share an outer post-content overlay; minimal reuses the same English painter, with reserved controls and capped viewport space. Minimal skips unused panel/tail layout while showing exit state; no new framework or duplicated exit protocol is introduced.

Previously, Pending keyboard tests and direct Retry action tests both passed while missing the Unconfirmed keyboard route between them. Minimal's independent draw path was outside the normal painter tests. New tests cover input→dispatch→failed result→two-key retry→stale response rejection→confirmed exit; partial confirmation followed by r/f; narrow-screen controls; and the minimal viewport/shared painter seam.

## Results

Root-owned local validation is recorded below. No hosted platform or installed-binary fix is claimed.

The old process log also contains prompt.enqueue after the suspension prefix, without a later provider turn in the inspected durable history. Input being accepted by the frontend is not evidence that suspended execution continued. Both reported process logs identify version 2.4.0.

## Local library results

Full Pager: **7,530 passed, 12 ignored, 0 failed**. The final three focused tests passed: input/result/retry identity, narrow error/control painter, and partial Quit followed by r/f. Minimal: **95 passed, 0 failed**, including shared notice sizing/control rendering. These results cover state and render seams; they do not claim a real user session was suspended during testing or that the actual pending physical owner in its old process has been identified. Logs: `/tmp/grow-suspension-pager.log`, `/tmp/grow-suspension-pager-final.log`, `/tmp/grow-suspension-minimal.log`.

## Final branch audit finding

A final review of every draw return found an additional missed early return in normal Agent rendering. Painter-only tests and the first proposed per-view calls did not cover it. The final normal renderer uses one outer exit-overlay tail around its content callback, covering every view return. An actual AppView::draw subprocess regression is added for Agent, Welcome and Dashboard; its red/green result follows. This correction is kept separate from claims about the physical suspension owner.

The actual-frame test failed before the final connection fix with **AGENT draw omitted exit error**; its child really rendered AppView through the fixed terminal and background writer. Red log: `/tmp/grow-suspension-actual-frame-red.log`. The final normal draw has one content callback and one outer overlay; branches no longer need individual exit calls. Terminal diff output can omit literal spaces via cursor positioning, so the captured text assertions compare whitespace-normalized phrases while preserving page boundaries and control words.

Final focused render/input run: **4 passed, 0 failed**, including the actual Agent/Welcome/Dashboard frames, narrow error/control painting, full retry/result identity path and r/f confirmation cleanup. Green log: `/tmp/grow-suspension-actual-frame-green.log`. The subsequent import qualification/format-only adjustment keeps the original renderer indentation and avoids a large whitespace-only diff; the CLI is rebuilt from the final sources below.

## Final combined checks

The CLI was rebuilt from the final formatted renderer sources. The exact runner then passed both root/child/grandchild suspension with cold restoration and minimal frozen-Skill cold restoration against that executable. Together with the four earlier process cases, this verifies the shared runtime changes and final normal/minimal composition. Final logs: `/tmp/grow-suspension-render-cli-build.log`, `/tmp/grow-suspension-final-process-tests.log`. Full pinned formatting and diff whitespace checks passed; the selected documentation scan covered **20 Markdown files / 197 relative targets / zero missing**. Strict OpenSpec validation passed **20 active/main items** with zero failures. No hosted-platform or release-dist run is implied.

At the implementation-validation checkpoint, the work was isolated and uncommitted; the archive/integration follow-up is recorded below. Main's 34 pre-existing tracked modifications and MCP work were not changed. No live-session input/control, install, tag or publication was performed.

The task-owned Cargo target (about 10 GiB) was removed after process validation completed. Diagnostic logs and the small read-only process sample remain in /tmp; unrelated targets and user processes were left intact.

## Archive follow-up — 2026-10-19

The four completed changes were archived with OpenSpec 1.11.0. The two maintenance/test-only changes used --skip-specs; the behavioral changes updated model-sampling and client-surfaces. Current developer links now point to the main specifications or dated verification archives. The runtime sources were not changed during this finalization; the red/green, library, real-process and measurement evidence above remains the implementation evidence. Current release readiness is summarized in the sibling recover-unconfirmed-quit-input release-analysis.md.

### Finalization checks

After archiving, strict main/active OpenSpec validation passed **16 items** and strict archive validation passed **625 items**, with zero failures. The archive CLI's non-blocking proposal-header notices were addressed in the archived proposals; the two skip-specs changes still contain no invented behavioral delta. Pinned `cargo fmt --all -- --check`, `git diff --check`, and all **16** Python maintenance/release-runner tests passed. Locked Cargo metadata matched all **57** local package identities to Cargo.lock; the product version remains 2.4.0.

The final documentation scan covered **107 Markdown files / 583 relative targets / 329 anchors**, with zero missing targets or anchors. External links and 123 historical absolute local-file/diagnostic references were excluded from this relative-link check. Release/build shell syntax and six workflow YAML files parsed successfully; the extracted release static contract checker passed. This does not execute hosted workflows or the complete tag/version/clean-tree release preflight. Release readiness and final Git/MCP preservation evidence are in [the release review](../2026-10-19-recover-unconfirmed-quit-input/release-analysis.md).
