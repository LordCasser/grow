# Implementation verification

## 最终运行验证 — 2026-10-19

当前分支 `codex/v2-4-0`，workspace/lockstep 和 CLI 目标版本为 **2.4.0**。已完成四项主能力、原 backlog 确认缺陷及联合验收发现的 headless event ID 修复。用户再次确认 Remote Control、Provider cache 和 Recap 方向保持暂缓；MCP 多轮同样暂缓且 SDK 门禁关闭。本轮没有 commit、tag、push 或发布。

最终包级回归为 Shell126 **4251 passed / 0 failed / 5 ignored**、Pager124 **7503 / 0 / 12**、Tools121 **2416 / 0 / 7**。日志分别为 `/tmp/grow-v2-4-0-shell-final126.log`、`/tmp/grow-v2-4-0-pager-final124.log`、`/tmp/grow-v2-4-0-tools-final121.log`。ChatState108 567/0/2、tool-types106 123/0、Agent110 482/0 及各 backend 的既有通过记录继续作为相应未改责任层的证据。CLI122 构建并报告 `grow 2.4.0 (8cc20454)`；后续修改仅为测试夹具，未更改生产语义。

当前真实进程验证共九项：CLI122 的 root、三层树、部分释放、并发恢复和普通 continue 五个 exact PTY 各 1/1（`/tmp/grow-v2-4-0-pty122-results.json`）；Plan approval 与 Defer 后冷恢复审阅为 2/2（`/tmp/grow-v2-4-0-plan-pty122.log`）；normal/minimal Skill 挂起、原快照恢复、导出和 headless 继续为 2/2（`/tmp/grow-v2-4-0-skill-surfaces123.log`）。

验收以生产 owner 的不变量为单位组合 Timeline、Actor、coordinator、存储故障注入、UI reducer 与真实进程证据。下方旧矩阵记录各阶段已运行和未穷举的排列，不再把缺一个巨型组合夹具当作实现阻断，也不声称每个排列都在实机执行。物理断电/满盘、所有状态交叉组合、Linux/Windows 实机和真实 MCP 多轮互操作未运行。MCP 多轮按用户决定暂缓，其门禁保持关闭。版本总体证据见 [联合验证](verification.md)。

最终对抗性审查按控制 owner 和失败边界核对，不依赖 suite 总数推导行为。原 backlog 的逐项确认、最小 change 和归档见 [backlog-audit.md](backlog-audit.md)。四项功能验证文件文首记录各自场景核销及未穷举范围。

Shell125 的三项失败保留在原日志：缺 journal 和错误使用缓存后 GROW_HOME 的两个夹具已修复；cleanup 资格测试未改生产条件，在定向、151 项子集及最终 Shell126 中通过。最终完整 Shell 运行使用临时 GROW_HOME，嵌套结果测试另外用独立子进程隔离路径缓存。Pager124 两项新增评论/Skill reducer 测试也逐项通过。

Rust/Rustdoc 固定 1.93.1，两编译任务、16 MiB test stack、禁用 incremental/dev/test debug，直接运行 no-run 生成的包测试二进制并使用四测试线程。CLI122、suite123、Pager124、Shell126 构建成功；CLI122 后的差异仅为测试夹具。组合 feature graph 额外的三个 testkit 测试另在 `/tmp/grow-v2-4-0-shell-feature-tests125.log` 3/3 通过，不重复计入 Shell 包总数。

格式和 diff 检查通过；归档前 strict 为 21/21（`/tmp/grow-v2-4-0-openspec-all126.log`），既有 archive 为 599/599。归档后的结果见下方“归档后校验”。已移除三份本轮旧 test executable 共 536.7 MiB，并清理 8 份本轮诊断 long-type 文件；保留共享依赖缓存，target 约 30 GiB，磁盘约 1.3 TiB 可用。旧存储格式不提供迁移。

## 归档后校验 — 2026-10-19

本轮六个 change 已归档，规范合并与引用修正完成。`openspec validate --all --strict --no-interactive` 为 **16/16**，`openspec validate --archived --no-interactive` 为 **605/605**，日志 `/tmp/grow-v2-4-0-openspec-all128.log` 与 `/tmp/grow-v2-4-0-openspec-archived128.log`。39 处迁移后的 Markdown 引用均可解析；六份 tasks 无未完成项。格式与 diff 检查通过。当前唯一 active change 为用户暂缓的 `add-mcp-multi-round-elicitation`。

## 历史阶段证据

以下保留各轮真实诊断和失败记录，较早的“未运行 / 仍受阻 / active”不代表最终状态。


## Current verified baseline / remaining — 2026-10-19

当前实现分支 `codex/v2-4-0`；workspace 与 CLI 目标版本为 **2.4.0**。CLI111 build 成功（`/tmp/grow-v2-4-0-cli-build111.log`）；此前 CLI73 的 `grow --version` 为 `2.4.0 (8cc20454)`，该 SHA 只是未提交实现基线。没有发布提交、tag、push 或发布。本节更新包级基线，不取代下方仍有效的历史限制与失败记录。

| 范围 | 已运行结果 | 日志及边界 |
| --- | --- | --- |
| Pager library | **7497 passed, 0 failed, 12 ignored** | `/tmp/grow-v2-4-0-pager-final111.log`；含 draft worker、Plan notes、offline gap 等具名回归。 |
| Agent library | **482 passed, 0 failed** | `/tmp/grow-v2-4-0-agent110.log`。 |
| Tools library | **2416 passed, 0 failed, 7 ignored** | `/tmp/grow-v2-4-0-core108.log`。 |
| Workspace library | 874 passed, 0 failed | 旧的独立运行 `workspace-32.log`；含实际 permission observer，本批未重跑。 |
| ChatState library | **567 passed, 0 failed, 2 ignored** | `/tmp/grow-v2-4-0-core108.log`。 |
| tool-types / sampler / sampling-types / pager-minimal | **123** / 259 / 308 / 94 passed, 0 failed | tool-types `/tmp/grow-v2-4-0-tool-types106.log`；另三包各 `*-32.log`。tool-types106 含 `planning::tests::child_handled_revision_is_checked_only_by_its_local_owner`。 |
| MCP library | 178 passed, 0 failed | 旧的独立运行 `mcp-final51.log`；服务端生效后丢回复不重放，本批未重跑。 |
| Shell storage | 362 passed, 0 failed, 1 ignored | 旧的独立运行 `storage42.log`；闭链回收、离线 capture，本批未重跑。 |
| Shell library | **4244 passed, 0 failed, 5 ignored** | `/tmp/grow-v2-4-0-shell-final111.log`，127.68s；最新完整 Shell suite。 |
| 历史挂起边界（focus97） | 3 tests passed, 0 failed | `/tmp/grow-v2-4-0-focus97-results.json` 与各 focus97 日志；root barrier lost ACK、blocked/full 不成功、Timeline prefix cold-load。三项也均在 ChatState full97 中具名 `ok`。 |

Shell111、Pager111、Agent110、core108 与 tool-types106 是当前各包已运行的通过基线。固定 Rust/Rustdoc 1.93.1、两编译任务、16 MiB test stack、禁用 incremental/dev/test debug；直接运行 Cargo no-run 生成的包测试二进制，四测试线程。CLI111 构建通过。Plan defer PTY 仍未完成；Skill 专项 PTY113 已通过，见下文；因此不能声称产品端到端或发布门禁全绿。旧 Shell50/62 和 Skill fault 夹具失败保留为诊断，未计入本节通过数。

真实进程证据包括 CLI73/harness76 的 PTY77 七项成功场景、Plan approval 冷恢复 `plan-pty78.log` 1/1，以及 CLI113/harness113 的 Skill 专项 `skill-pty113.log` 1/1；这些是各自构建产物的具名场景，不替代包级测试或完整发布门禁。Skill PTY 真实验证 user-only Skill 加载、命令运行、Ctrl+C 后 CLI 正常退出并挂起会话、修改磁盘 Skill 后同 session/Turn 冷恢复；原正文在两次请求中保留且新正文不泄漏，load 生命周期一次、命令 started=1/finished=0、Interrupted 一次。先前两次 Skill PTY 失败均为 fixture 错误：一次未保留源正文尾换行，另一次在挂起前读取 timeline；修正后未放宽产品断言并于113通过。Plan defer `plan-pty111.log` 中 `plan_approval_restored_after_resume` 通过，但 `deferred_plan_reviewed_after_resume` 因 30 秒内未重新出现 “request changes” 超时而失败，仍未完成。跨平台实机矩阵及真实 MRTR 互操作仍未运行。历史 PTY 确认范围只限其具名场景。

本轮已核对并归档十二项最小修复：来源询问启动去重、Search epoch 隔离、未知 stop reason、Workflow clear revision、Plan 部分 notes、child MCP 策略刷新、闭链联合回收、不确定 MCP 调用禁止重放、session bind 期间保留输入、drain 已取消的后台 compaction、检测 transcript 展示缓存缺口、记录人工等待与挂起区间。原 backlog 的逐项代码分析见 [backlog-audit.md](backlog-audit.md)；Remote Control 明确暂缓，Provider/Recap 缓存收益仍是缺少真实 route/负载证据的假设。MCP 多轮按用户决定暂缓，SDK 门禁关闭。

对本轮 backlog audit 中 12 个归档记录目标做了定向存在性检查：11 个唯一 Markdown 相对链接（文中共 12 处引用，含 1 处重复）和 `unify-session-mcp-server-identity` 记录均能解析到对应归档目录及 `verification.md`；缺失 0 项。`refresh-live-mcp-auxiliary-policy` 是另一个归档依赖记录，不计入这 12 个 backlog 修复目标。

各功能 change 的场景矩阵独立核对，不能用全量计数替代未注入的故障。Plan defer PTY 仍需修复/复测；尚未完整验收的 change 保持 active。Skill PTY 的一次夹具修正已由 PTY113 验证通过，不再列为待复测门禁。最终格式、strict、archive 和磁盘核对在剩余验证完成后统一执行。Linux/Windows 实机矩阵及真实 MRTR 互操作未运行，本记录不声称可发布。

## Environment and scope

- Workspace: macOS arm64, implementation branch `codex/v2-4-0`; starting HEAD `8cc20454`, starting version 2.3.1, target 2.4.0.
- Preserve the initial worktree manifest `/tmp/grow-v2-4-0-initial-worktree.json`. Initial null usage fixes and the separately archived text-only rejection change were not reverted.
- User explicitly deferred MCP multi-round interaction. Its existing capability gate remains closed; no real Form/URL interoperability service is available.
- A separate concurrent task added `harden-image-fallback-routing` during implementation and archived sampling regression work. These later changes are preserved and are not automatically added to this release's original implementation scope.
- Cargo runs are serial, with `CARGO_BUILD_JOBS=2`, explicit Rust/Rustdoc 1.93.1, incremental and dev/test debug information disabled. Unqualified Cargo checks from the initial implementation used the host's different compiler and are not counted as pinned-toolchain evidence.

## Historical build and diagnostic records

- Pinned `cargo test --locked -p tool-types -p chat-state -p tools --lib`: **ChatState 560 passed / 2 ignored; tool-types 121 passed; tools 2412 passed / 7 ignored; zero failures**. Log: `/tmp/grow-v2-4-0-core-tests.log`. Later summary/fixture additions still need verification.
- The newly built Shell test executable passed `runtime_restore_tests` under the documented 16 MiB test stack: original accepted response, Turn, Request and completed Stop gate remain unique; no provider/prompt replay. Log: `/tmp/grow-v2-4-0-shell-restore-run.log`. This is an actor-level check, not cross-process acceptance.
- The first full Shell run omitted the repository's test-stack setting and aborted on stack overflow. A subsequent diagnostic run was stopped after exposing legacy fake input payloads and old TODO arguments. Two new Skill pipeline tests also failed preflight; these are unresolved and are not counted as passes. Logs: `/tmp/grow-v2-4-0-shell-tests.log`, `/tmp/grow-v2-4-0-shell-binary-tests.log`, `/tmp/grow-v2-4-0-skill-pipeline-run.log`.

- `cargo test --locked -p tool-types planning::tests` with the pinned toolchain: **18 passed**, 0 failed. Covers effective diff rules, coalescing vs immutable dispatched batches, old acknowledgements, tombstones, required review, scope obligations, and transition validation. Log: `/tmp/grow-v2-4-0-planning-tests.log`.
- `cargo test --locked -p chat-state --lib` with the pinned toolchain: **554 passed**, 0 failed, 2 ignored. This run includes source-hash checkpoint validation, preserved logical Turns, conditional User/Skill publication, model-call identity validation, unconsumed Skill compaction protection and terminal settlement. Earlier runs exposed invalid new test fixtures (missing input Hook/Turn identity and unfinished compaction scopes); those fixtures were corrected without relaxing production validation. Log: `/tmp/grow-v2-4-0-chat-state-tests.log`. Later source changes still require their own verification.
- Earlier pinned `cargo check --locked -p shell -p pager --lib` passed before subsequent Skill, approval, and suspension integration changes; it is not a final build result.
- Current all-target checks are still in progress. They exposed enum consumers and an old MCP test fixture using the removed `rmcp::model::Meta` name; the fixture is being updated to the installed SDK's `MetaObject`, without enabling the deferred interaction feature.
- Last disk check: target 8.3 GiB, approximately 1.4 TiB free. Final cleanup has not occurred.

## Historical release-gate snapshot

No feature is declared complete by this progress record. Actor/coordinator fault cases, user Skill publication and cancellation, retained Plan acknowledgement, same-identity tree restoration, Workflow handoff, exit UX, cross-process PTY scenarios, final crate checks, documentation, version metadata, archive and full strict validation remain to be verified. Unchecked feature tasks must remain unchecked until their actual scenarios pass.

## Integration pass — not yet final

- Full pinned Shell suite with the required 16 MiB stack: **4,119 passed, 13 failed, 5 ignored**; `/tmp/grow-v2-4-0-shell-final-suite.log`. Both cross-process catalog-route and effort recovery regressions passed after the fake client correctly acknowledged replay notifications. Failures are retained in the log; later fixture and Skill assertion fixes have not yet passed a final rerun.
- The user Skill pipeline now reaches its provider request and completes. The earlier cancellation was a real integration defect: a direct Runtime Step end bypassed the normal control boundary. Preparation now opens a Runtime Step only when explicit Skill references exist, and the regular model boundary/turn terminal closes it. The request-body assertion still needed to decode the protocol's JSON result string to inspect the exact prepared body.
- Model Skill publication now uses the active prompt coordinate rather than the next prompt index. Its HTTP-failure test now bounds the session recovery budget so a fallback mock response cannot hide the intended failure. Both changes require the next test run.
- Suspension reconciliation now refreshes canonical child terminals before committing the tree barrier, preserves actual child retirement responsibility, and excludes verified completed nodes from restore. The StructuredOutput continuation retains its synthetic call and retry budget across a freeze, reusing an exact canonical ToolResult instead of issuing another provider request. These new boundaries still require integration verification.
- Executing resume from a suspended child now uses the canonical root identity in Pager and headless clients. CLI build and the real root/nested-tree PTY tests are pending.
- Version metadata is now **2.4.0**, with a draft changelog. No release tag, publication, feature archive or release completion is claimed.

## Final core regression — 2026-10-18

Pinned Rust/Rustdoc 1.93.1, 16 MiB test stack, two build jobs, four test threads, incremental and debug info disabled:

`cargo test --locked -p tool-types -p chat-state -p tools --lib -- --test-threads=4`: **ChatState 561 passed / 2 ignored; tool-types 122 passed; tools 2412 passed / 7 ignored; zero failures**. Log: `/tmp/grow-v2-4-0-core-final-tests.log`. Includes the later runtime command evidence, bounded child summary, structured continuation schema and Skill publication changes. This closes this core package run; it does not replace pending Shell/Pager and process-level verification.

Release integration exposed a pre-existing input checkpoint failure at CWD→session binding, recorded in the necessary prerequisite `preserve-input-during-session-bind`. The old worker restore could replace the freshly dequeued immutable in-flight input. The fix retains the existing durability gate and has a real worker regression. A separate selected-Skill gap is fixed within `unify-skill-loading`: queued structured requests must retain an in-flight intent and their frozen name/path/plugin source through checkpoint/retry. Post-fix Pager and CLI verification is pending.

## Later integration findings — verification pending

- CLI pinned build passed (`/tmp/grow-v2-4-0-cli-build6.log`); `grow --version` reports `2.4.0 (8cc20454)` without implying an implementation commit. The real `continue_resumes_session_with_history` PTY now passes (1/1, `/tmp/grow-v2-4-0-pty-baseline-after-bind.log`).
- Full Shell: **4149 passed, 2 failed, 5 ignored** (`/tmp/grow-v2-4-0-shell-regression4.log`). Four Skill pipeline cases, retained Defer→Control reconstruction→Review→real approval waiter→single handoff, source-drift gating and StructuredOutput continuation tests pass. The remaining failures were new fixtures using an unrelated Turn ID and invalid Plan link/step responsibility; fixes require rerun.
- Root PTY demonstrated actual confirmed suspension, persisted Interrupted command, original-identity cold restoration and resumed provider output after fixing post-load orphan recovery. It then failed an incorrect test helper: canonical user text is in InputConsumed, not a direct-user Messages event. Neither that failed run nor the earlier zero-request failures are counted as passing suspend acceptance.
- Adversarial review found and fixed stale root facts after awaited-child completion, post-release cold recovery closing the live Turn, root-first release hiding unreleased descendants, StructuredOutput bypassing dependency wait, Replan ACK failures losing retry scheduling, and pending selected Skill drafts losing their source. A typed partial-leaf-release PTY fixture is added. The final source still needs its complete test/build pass.

## Verified client and sampler packages — 2026-10-19

- Pinned `cargo test --locked -p pager --lib -- --test-threads=4`: **7477 passed, zero failed, 12 ignored**, `/tmp/grow-v2-4-0-pager-regression4.log`. Includes the real draft worker/session binding and selected-Skill in-flight/staged-source tests. Previous seven failures were corrected without weakening canonical identity or approval rules: stale waiter teardown closes the transport without inventing a user decision; ordinary SKILL.md reads remain normal file reads; OS quit fixture has a Tokio reactor; Dashboard coalescing fixture owns an executable root.
- Pinned `cargo test --locked -p sampler -p sampling-types -p pager-minimal --lib -- --test-threads=4`: **sampler 259 passed; sampling-types 306 passed; pager-minimal 94 passed; zero failures**, `/tmp/grow-v2-4-0-sampling-minimal-tests2.log`. Minimal's old coordination test literal was updated to the concurrently renamed `source_session_id`; no inquiry behavior was changed by that fixture fix.
- Root real-process suspension/resume passes with one command start, no finish/replay, original Turn/input identity and unique Interrupted evidence. `/tmp/grow-v2-4-0-pty-suspend-final2.log` is **1 passed / 1 failed**: the nested fixture had used an unsupported CLI enable flag, corrected for its next build. The original nested fixture also used an obsolete model-facing tool name; it now requests registered `spawn_subagent`/`background`, and explicitly configures the required depth and child permission route. These are not yet nested acceptance passes.
- The next Shell pass must include two real integration fixes: persist replan Consumed before the local-plan+Handled transaction, and retain the active Step through dependency wait so the normal post-wake boundary can continue it. The real StructuredOutput mock-provider test is the release gate for that second path.

## Planning causal-boundary fixes — 2026-10-19

- `structured_output_completion_waits_for_live_question_and_permission_without_resampling` passed with the pinned toolchain (`/tmp/grow-v2-4-0-structured-wait-focused2.log`). The active Step now survives dependency wait and is closed by the normal next model boundary, so waking does not incorrectly cancel the original Turn.
- `linked_parent_update_is_dispatched_received_handled_and_completion_is_not_replanned` passed (`/tmp/grow-v2-4-0-replan-focused-final2.log`). Both the child's local update and the parent's receipt handling must persist Received → Consumed before Handled. Receipt processing can resume from an already durable intermediate stage; four unconfirmed attempts retain the same request identity. Completion-only parent updates emit no additional replan.
- The first receipt-focused rebuild encountered an in-progress external MCP catalog method rename (`/tmp/grow-v2-4-0-replan-focused-final.log`). That compile failure is retained as diagnostic evidence; the subsequent successful rebuild includes the completed external method. It does not enable the deferred MCP multi-round feature.
- Full Shell regression and final CLI/PTY builds are running after these fixes. The additional matrix tests being prepared have not yet run.

## Portable Skill wire and Tools regressions — 2026-10-19

Pinned Rust 1.93.1 / two jobs / 16 MiB test stack / no incremental or debug info: `cargo test --locked -p sampling-types -p tools --lib` passed **sampling-types 308/308** and **tools 2415 passed, 0 failed, 7 ignored**. Log `/tmp/grow-v2-4-0-tools-sampling6.log`; tools completed in 28.43s. The two extra one-test summaries inside the log are child-process test invocations, not additional independent package totals.

The new backend tests `user_skill_load_projects_one_complete_call_result_pair_on_every_backend` and `model_skill_load_keeps_only_its_real_tool_exchange_on_every_backend` verify complete paired projection across Chat Completions, Responses and Messages without duplicating the model tool exchange. The strict invalid-UTF8 Skill read and Plan partial-action notes formatting regressions also passed. An earlier build failed in new test fixtures on Arc/string ownership; those fixtures were corrected before this successful run. This result does not replace Shell actor or real-process gates.

## Timeline27 core regression — 2026-10-19

`cargo test --locked -p chat-state -p tool-types --lib` with pinned Rust 1.93.1, two jobs, 16 MiB stack and no incremental/debug info: **ChatState 562 passed, 0 failed, 2 ignored; tool-types 122 passed, 0 failed**. Log `/tmp/grow-v2-4-0-chatstate-backlog8.log`. The preceding run failed only because the architecture document still named Timeline26; that document was synced to Timeline27 before this run. Counts alone do not prove Shell/UI behavior.