# Verification

## Scope and source

The hardening worktree originally based on `cd5d88ec` was integrated onto committed main `4a560b63`, retaining `f99310f7` and `4a560b63` Plan behavior and tests. All eight implementation changes were archived with `openspec archive <name> --yes`; the appended-spec conflicts retained both sets of requirements and backlog entries. Current guidance points to the main specifications, and sibling verification references point to dated archive locations.

This record supplements, rather than replaces, the earlier platform/toolchain evidence in those eight archives. It does not claim hosted Windows/Linux acceptance or publication. No MCP source or protocol changes are included.

## Prompt-size regression caught during integration

The first complete pinned-toolchain gate failed three Agent size assertions: child/general-purpose 8672 bytes against 8192, read-only 8197 against 7680. The earlier write-location change had used static validation and had not rerun Agent's existing composition budget tests after adding a 550-byte paragraph. That old evidence did not cover the final assembled prompt.

The added paragraph was reduced to 373 bytes without dropping the default project/worktree location, platform temp directory, explicit/necessary external destination, necessary-file limit, existing safety/authorization, or the exclusion of convenience/absolute-path/access-only justification. Both size limits reserve an additional 512 bytes for that new shared safety guidance; assembled prompts are 8495 and 8020 bytes. Tests continue checking total rendered size and the read-only/full relationship. No original audience, role, permission or template composition logic changed. The subsequent Agent library run passed all 482 tests, including the three original failing assertions.

## Documentation and static release analysis

- The relative-link scan initially found one old `improve-dependency-driven-agent-prompts` active-path reference in PROMPT_ARCHITECTURE. It now points to the archived verification and describes the delivered guidance and actual evaluation limits.
- Current docs, Pager guide, main specs, active changes, this round's archives and prompt architecture: **137 Markdown files, 525 relative link targets, 259 requirement anchors, zero missing targets or anchors** at the initial completed scan. The final scan is recorded below after this verification file is included. Absolute historical source/log paths are outside this relative-link assertion.
- Python exact-runner/release-DAG suite: **16 passed**; includes a real temporary Cargo/libtest fixture and negative selectors. YAML parsed for core, coordination, Windows storage, OpenSpec, release and build-one.
- The embedded static contract check from `scripts/validate-release.sh` passed against the existing 2.4.0 notes: exact ten-platform catalog and publication sets, updater repository, attestation workflow and build/signing contracts. This is not the full clean-tree/annotated-tag entry point and does not establish 2.4.1 readiness.
- Pinned Cargo metadata: **57 local packages** match lockfile identities/versions; **33 workspace-version packages remain 2.4.0**. The 24 independently versioned libraries remain unchanged. A 2.4.1 notes file and version bump are intentionally left for the separate release preparation step.
- Read-only remote inspection confirmed public non-prerelease v2.4.0 with ten archives plus SHA256SUMS, remote main/tag at cd5d88ec, and no v2.4.1 tag at inspection time. See release-analysis.md for the candidate scope and remaining platform/configuration coverage boundaries.

## Main work preservation

Before main integration, its 36 modified tracked files and 17 untracked files were copied with SHA-256 metadata into `/tmp/grow-241-main-preserve`, together with the original HEAD and binary diff. No source was hidden or removed during that backup. The resulting main integration and preservation checks are recorded below after completion.


## Combined library gate

Pinned Rust/Cargo 1.93.1 on macOS arm64, four build/test workers, 16 MiB test stack, no incremental build, dev/test debug info disabled, isolated `/tmp/grow-241-main-integration-target`:

`cargo test --locked --lib -p chat-state -p sampling-types -p sampler -p memory -p workflow -p shell -p pager -p pager-minimal -p agent -p tools -p tool-types -- --test-threads=4` passed **16,425 tests**, **0 failed**, **26 existing ignored**. Package counts: Agent 482, ChatState 567, Memory 302, Pager 7528, Minimal 94, Sampler 259, sampling-types 308, Shell 4275, tool-types 123, Tools 2422, Workflow 65. The nested one-test outputs from Tools are not counted a second time. Log: `/tmp/grow-241-main-libraries-final.log`; the original red run remains `/tmp/grow-241-main-libraries.log`.

`cargo fmt --all -- --check` passed. Pager emits an existing dead-code warning for two prompt-status helper methods; it is not a test or build failure and was not expanded into unrelated cleanup.

## Main integration preservation result

The implementation commit `404aaa79` was fast-forwarded onto main. Its temporary tracked-work stash was applied with explicit conflict resolution. All **31 non-overlapping tracked files and 17 untracked files are byte-identical** to the backup; the MCP additions in the developer guide and specification remain intact, and backlog preserves both the independent debt scope and the existing deferred-direction decision. The three overlapping documentation files remain uncommitted.

The other two pre-existing modified files contained only obsolete partial image-recovery UI additions, with an old two-argument/no-prompt interface and unconditional terminal settlement. They were reconciled to the complete tested implementation, which uses exact-prompt settlement and the current typed lifecycle. Retaining both would either fail compilation or invalidate stale-terminal behavior. These were not MCP changes; their original bytes remain in the recovery snapshot. Main now has **34 modified tracked files**, all outside this commit's pending work, with an empty index. Details: `/tmp/grow-241-main-preserve/preservation-result.json`. No MCP file was committed, and no untracked MCP archive or fixture was added to the hardening commit.


## Built binary and process validation

`cargo build --locked -p cli --bin grow` passed with the same pinned 1.93.1 environment. The built entry point reports `grow 2.4.0 (404aaa79) [stable]`; no version change is implied. Build log: `/tmp/grow-241-main-build.log`.

All **eight** selected cross-layer cases passed through `scripts/run_exact_cargo_test.py`, with both `GROW_BINARY` and `PAGER_BINARY` pointing to that just-built executable. The runner uniquely lists and verifies an actual one-test pass for each selector:

- `protocol_failure_preserves_session_across_continue_switch_and_process_restart`
- `malformed_wire_json_allows_manual_continuation_in_the_same_resident_session`
- `goal_sampling_failure_requires_explicit_restart_and_preserves_budget_gates`
- `plan_approval_restored_after_resume`
- `deferred_plan_reviewed_after_resume`
- `suspend_nested_task_tree::ctrl_d_suspends_and_cold_resumes_root_child_grandchild_tree`
- `ctrl_d_suspend_and_continue::ctrl_d_cold_resume_keeps_the_original_explicit_skill_snapshot`
- `ctrl_d_suspend_and_continue::minimal_cold_resume_keeps_the_original_explicit_skill_snapshot`

The protocol/Goal cases retain their multi-protocol/delivery/budget subcases and real resident actor lifecycle. The delayed-ACK child-grant cases are part of the complete Shell library run. The PTY cases cover real suspended trees, approval state and unchanged admitted Skill bodies, without direct Idle resets or replaying interrupted commands. This selected local set does not claim every PTY integration in the hosted core workflow was run. Log: `/tmp/grow-241-main-integrations.log`.

`scripts/test_local_coordination.py --binary <just-built grow>` passed all **ten** independent-process checks: discovery, busy foreground/FIFO/deduplication, failure/isolation, cancellation/approval, actual list/ask tools, durable restart audit, crashed-target lease expiry, peer identity, resident reload and interrupted terminal receipts. Log: `/tmp/grow-241-main-coordination.log`.

The final Python suite was rerun with the pinned Rust/Cargo 1.93.1 PATH: **16 passed**. The read-only release-analysis review found no contradictory readiness or platform claims. Hosted release gates and release-dist artifacts remain unexecuted in this local task.

## Final checks before maintenance archive

Pinned formatting, diff whitespace and the final relative-link scan passed: **138 Markdown files, 526 relative targets, 259 requirement anchors**, zero missing targets or anchors. OpenSpec 1.11.0 validated all active/main items (**17 passed**) and archives (**620 passed**). The shell's Cargo-only PATH does not contain the npm OpenSpec executable, so the initial post-test CLI invocation did not run; validation was then explicitly rerun in the normal OpenSpec environment. The failed invocation is not counted as validation evidence.

The maintenance record uses skip_specs because it adds no behavior contract. Its archive and post-archive checks follow this entry. The temporary preservation stash was removed after the byte/content checks; the small original-file backup remains in /tmp. No push, tag or release was performed.

## Post-archive result

The eight implementation archives and this maintenance archive are complete. Post-archive OpenSpec strict validation passed **16 active/main items** and archived validation passed **621 items**, with zero failures. The post-move relative-link/requirement-anchor scan remains **138 files / 526 targets / 259 anchors**, zero missing. Corrected the bound-agent-file-write-locations verification link to this dated archive. All maintenance tasks are checked only after their corresponding work and checks.

Removed the task-owned `/tmp/grow-241-main-integration-target` (about 12 GiB) and generated Python bytecode caches after process tests completed. Main's pre-existing Python cache and other worktrees/targets were left intact. Diagnostic logs and the small preservation backup remain outside Git. The final follow-up commit contains only documentation and this OpenSpec maintenance archive; runtime validation applies to the implementation commit 404aaa79.
