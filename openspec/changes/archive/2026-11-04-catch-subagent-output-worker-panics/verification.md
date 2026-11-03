# Verification

| Area | Result | Evidence and boundary |
| --- | --- | --- |
| Tools crate | 2441 passed / 0 failed / 7 ignored | Final `tools` library result in `/tmp/grow-260-final-core5.log`. |
| Observation coordinator cases | 4 passed / 0 failed | The final tools run reports `active_discovery_distinguishes_forgotten_id_retired_checkpoint_and_lost_owner`, `completed_output_readers_are_bounded_and_release_their_permits`, `persisted_output_worker_panic_does_not_destroy_the_coordinator_owner`, and `public_active_discovery_distinguishes_forgotten_from_finished_children` as `ok`. The injected panic is expected test input; later coordinator work remains available. |

The containment claim applies to an unwind from the persisted-output reader future. It does not claim recovery from process abort: `Cargo.toml` sets `[profile.release] panic = "abort"`, inherited by `release-dist`. The test demonstrates the unwind boundary in the Rust test binary only.

Earlier fixture failures and corrections remain in the [umbrella verification](../../close-v2-6-0-release-blockers/verification.md); the final core run supersedes the prior local Tools result. Candidate-wide core results are 19233 passed / 0 failed / 51 ignored across 18 crate blocks; workspace check passed in 1m30s without warnings, Settings E2E passed 235/235, strict pre-archive validation passed 26/26, and the ordinary local release build passed in 3m21s. Archival, the required native cross-platform matrix and official publication remain incomplete.

## 归档收尾（2026-11-04）

已执行 `openspec archive catch-subagent-output-worker-panics --yes`，相应 delta 已合入主规范；纯维护/调查 change 按 skip_specs 归档。最终本机结果见上表，跨平台 CI 与官方发布仍由[发布闭合记录](../../close-v2-6-0-release-blockers/verification.md)跟踪。归档后的严格校验日志为 `/tmp/grow-260-postarchive-active-final.log` 和 `/tmp/grow-260-postarchive-archived-final.log`。
