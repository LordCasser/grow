# Verification

| Area | Result | Evidence and boundary |
| --- | --- | --- |
| Auxiliary routes | Passed | Final Shell core run `/tmp/grow-260-final-core5.log`: `auxiliary_suggestion_and_memory_rewrite_use_catalog_provider_routes` reports `ok`. It covers the three configured entry points (AI shell suggestion, prompt suggestion, and memory rewrite), including exact catalog resolution and distinct endpoint/backend/credential behavior. The Shell crate finished 4576 passed / 0 failed / 9 ignored. |
| AI suggestion sideband across API backends | Passed | `ai_suggest_stream_records_terminal_and_raw_body_in_its_sideband_attempt` reports `ok` in the final Shell core log and in `/tmp/grow-260-final-target-ai_suggest_stream_records_terminal_and_raw_body_in_its_sideband_attempt5.log` (1 passed). The test iterates Chat Completions, Responses, and Messages and checks the corresponding request path and sideband evidence. |

An earlier isolated auxiliary-route run failed because its workflow sampler fixture had a zero context window; that fixture was corrected. The final core run passes the route test. Candidate-wide core results are 19233 passed / 0 failed / 51 ignored across 18 crate blocks; workspace check passed in 1m30s without warnings, Settings E2E passed 235/235, and strict pre-archive validation passed 26/26. Ordinary local release build passed in 3m21s; it is not an official distribution build. The earlier failure remains in the [umbrella verification](../../close-v2-6-0-release-blockers/verification.md) as history. Native platform CI, archival and official publication remain separate gates.

## 归档收尾（2026-11-04）

已执行 `openspec archive resolve-auxiliary-suggestion-model-routes --yes`，相应 delta 已合入主规范；纯维护/调查 change 按 skip_specs 归档。最终本机结果见上表，跨平台 CI 与官方发布仍由[发布闭合记录](../../close-v2-6-0-release-blockers/verification.md)跟踪。归档后的严格校验日志为 `/tmp/grow-260-postarchive-active-final.log` 和 `/tmp/grow-260-postarchive-archived-final.log`。
