# 已合并响应恢复提交复核

审查日期：2026-09-17。`git fetch origin` 后，本地 `main`、`origin/main` 均为 `c17b085d07f8d6fa3f358207b02efdd3cc6352a8`（v2.1.10）；工作树起始干净。

范围：`bd1f5085..c17b085d` 中的 response admission、response replay projection、session/load、cursor、writer repair、fork 和 rewind。主要提交为 `ece63681`、`40f54d01`、`795f4b75`、`6d4c0138`、`78d43c01`、`766d32c4` 及其测试提交。本次只审查和验证，不修改运行时实现。

已读 OpenSpec 索引、主规范相关要求、`reconcile-response-replay-projection` 的 proposal/design/delta/tasks/verification、当前开发说明及生产调用方。该 projection change 仍未归档，tasks 全部未勾选；其 delta 是本次实现意图的证据，不能冒充已经归档的主契约。

## 结论

有四项需要修复的问题。前三项已用当前 ChatState 编译产物和直接包含当前 `response_projection.rs` 的隔离 Rust harness 复现；第四项由完整错误传播路径确认，尚未做真实 fsync 故障注入。以下问题都不能由现有单次 rewind、delta-only 或 post-commit bookkeeping 测试排除。

## R1 / P1：第二次 rewind 丢掉仍被保留的 response admission

位置：`crates/codegen/chat-state/src/timeline.rs:6566`，引入于 `ece63681`。

`reset_rewind_branch` 用旧 `leaf_birth` 找到保留的 admission event，然后调用 `reset_branch`。后者把所有保留条目的 birth 重建成当前 rewind event；函数只恢复 `admitted_response_events` 集合，没有保留这些 admission 到新叶子的归属。下一次 rewind 读取到的是第一次 rewind 的 event seq，再与原 admission event seq 求交集，原 response 就被删掉。

复现：p0 → answer A → p1 → answer B → rewind p1 → p1 replacement → answer C → 再次 rewind p1。两次 rewind 后 answer A 都在 Surface 中，但 `admitted_responses()` 从 1 变为 0。共享 replay planner 随后把 answer A 的完整 projection 当成 inactive cache 抑制。模型仍能看到 A，重载和导出却看不到 A。

回归应覆盖两次以上真实 rewind，并同时断言 Surface、admission view、typed/raw replay 保留同一前缀；还需覆盖夹杂 compaction 的重复 rewind。修复需要保留跨 rewind 的响应 provenance，不能只让 replay 放行无法验证的记录。

## R2 / P1：fork 复制了父 projection，却没有复制它的 authority

位置：`crates/codegen/shell/src/session/storage/jsonl/mod.rs:3011`，projection 复制分支引入于 `ece63681`；配套调用在同文件 `copy_session`。

普通 fork 在 `Timeline::from_seed(surface_to_copy)` 中建立新 lineage。继承的 Assistant 成为 seed，不含父会话的 `response_admission`。但 `transform_session_id_in_update` 对新的 projection record 只改通知中的 SessionId，保留父 request/attempt/event/digest。fork 的首次 reconciliation 找不到这些 identity，于是抑制所有继承的 projection。

复现结果：fork Surface 中 Assistant 数量为 1，admission 数量为 0，复制的 projection 为 1，reconciliation 后为 0。这是当前版本新建 fork 的问题，不依赖旧格式兼容。父会话模型上下文继承成功，并不意味着 fork 的可见历史也继承成功。

回归应通过真实 `copy_session` 创建普通 fork，再分别走 session/load 和 direct replay，断言继承的 text/thought 恰好出现一次。修复应明确 fork 历史展示的归属，不能把父 admission identity 直接当成子 Timeline 的 authority。

## R3 / P1：resident reconnect 初次快照会吞掉并发完成的响应

位置：`crates/codegen/shell/src/session/response_projection.rs:178`。基础逻辑引入于 `ece63681`，`6d4c0138` 只给 `AlreadyEmitted` delta 增加了放行例外。

生产顺序是：`acp_agent.rs` 静音 resident gateway，flush，`load_light` 取得 Timeline 快照；稍后 `replay_session_updates` 再读取 updates 文件。静音不暂停 resident actor。若一个新 response 在这两次读取之间完成 admission 和 projection append，新 projection 已进入物理 updates 快照，却不在较早 Timeline 中。

初次 replay 使用 `Reconcile`，把这个新 projection 当成 inactive/rewound 数据删掉，同时 `end_offset` 已越过它。第二次 flush 后的 delta 从该 offset 继续，也不会再读到它。gateway 静音期间的 live 通知不能补回这个空洞，用户会缺一段已经完成的回复或只看到其后继工具更新。

局部复现：旧 Timeline 快照 + 新完成响应的一个物理 projection，初次 reconciliation 输出 0；物理 delta 为空，后续输出仍为 0。尚未用完整 ACP reconnect 并发 harness 注入该调度窗口。

回归必须在 Timeline snapshot 与 updates snapshot 之间设置真实 barrier，让 resident turn 完成，再断言 load 完成前新响应和后继工具按序出现一次。需要一个相互一致的 authority/cache cutoff；仅在 delta 放行未知 identity 不足以覆盖初次快照。

## R4 / P1：把写后可读误当成 durable ACK，吞掉 sync 失败

位置：`crates/codegen/shell/src/session/storage/jsonl/mod.rs:3479`，引入于 `ece63681`；`append_acp_event_exact` 有相同问题。

底层 `append_jsonl_line_in_directory_sync` 的顺序是 write_all → flush → sync_file_durable → directory.sync。文件或目录 sync 失败时，完整 JSONL 行仍可能可读；`append_update_with_bookkeeping` 将该错误作为 `NotCommitted` 返回。新的 `commit_response_projection` 却把所有 append error 都交给 `raw_projection_status`，只要读到 exact payload 就返回成功，且后续 exact 分支也不会补做 sync。

因此完整 payload 的可见性替代了持久化确认。projection gate 会 ACK，Accepted、后继采样和工具可以继续。若随后发生机器故障，cache 可能丢失；因为后继工具或 request 已经存在，冷恢复的 tail-safe 检查还可能拒绝补齐该 response。

现有 `exact_projection_reconciles_a_post_commit_bookkeeping_failure` 模拟的是 durable append 已成功后 summary 写失败，不能证明 write 后 sync 失败同样安全；pre-commit append probe 又在写入前返回，覆盖不到该窗口。

修复应区分已确认 durable 后的 bookkeeping 错误和 durability 未确认错误。Exact payload 可以用于去重，但返回成功前仍需完成必要的文件和目录持久化屏障。回归应注入 write 后 file-sync 与 directory-sync 失败，并断言 gate 不会仅因可读而放行。

## 反证与范围外观察

- 空 response 初步疑点已排除：Messages、Chat Completions、Responses 的生产转换均无条件保留一个 Assistant item，包括没有文本的拒答。因此本次不把 `items.is_empty()` 的 guard 不对称列为生产缺陷。
- 旧版本 accepted candidate 缓存带 sampling identity，但 Timeline 没有新 admission metadata；当前 planner 会无条件抑制这种行。隔离 harness 也复现了 1 → 0。考虑项目明确不以向后兼容为目标，此项单独保留为观察，不计入本次四项修复优先级；若继续承诺 projection design 中的 legacy 原样保留语义，需要另行明确处理策略。
- Atlas 已成功打开本项目，文件范围 search 完成；后续 callers 查询长时间未返回，停止等待后以源码和真实调用方核对，不宣称得到完整仓库调用图。

## 局部复现证据

隔离 harness 位于 `/tmp/grow-response-review-repro.rs`，通过 rustc 链接当前 `chat_state` / `sampling_types`，并用 `#[path]` 直接包含工作树中的 `response_projection.rs`。Timeline fixture helper 取自该版本既有测试。只为该模块提供 SessionUpdate 容器，不替换 projector 或 planner 算法；不修改仓库 Rust 文件。

输出：

```text
first rewind: admissions=1, retained in surface=true
second rewind: admissions=0, retained in surface=true
resident replay: physical new projections=1, output=0
resident replay: next physical delta empty, emitted=0
fork replay: seeded surface assistants=1, admissions=0, copied projections=1, replay projections=0
legacy accepted replay: input=1, output=0
```

这些输出证明相应 fold/reconciliation 边界的行为，不冒充完整 fork、终端 UI 或 resident reconnect 端到端测试。定向 Cargo 验证结果记录在本 change 的 `verification.md`。

## 修复跟进

R1–R4 已在 `fix-response-recovery-boundaries` 中完成修复并归档。实现、回归与验证边界见[修复验证记录](../../archive/2026-09-17-fix-response-recovery-boundaries/verification.md)。以上审计发现及初始复现保留为修复前证据。
