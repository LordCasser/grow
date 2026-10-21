# 验证记录

## 失败基线

先运行回归再修复：chat-state 的真实 sibling 成功结果断言失败；sampling-types 的 5 个初始探针中 4 个失败（Responses 收件模拟调用、Chat Runtime 空字段、portable 真实结果缺失、prefix 被元数据截断），此前 Responses 用户技能修复对照通过。

## 验证范围

- sampling-types 库：315 passed。
- sampling-types `runtime_projection`：9 passed。覆盖三协议、全部 portable prefix 切点、native Messages 结果批次、Chat live/portable 与 backend 隔离、Responses 恶意正文及完整身份、此前用户技能修复。
- chat-state 库：573 passed，3 个既有 ignored。覆盖 Runtime-only 恢复一次性、路由更换、真实 actor 技能发布和冷恢复。
- actor 真实批次增强回归：1 passed。重复 metadata 发布拒绝且不追加事件；旧 IntegrityRepair 可重放；伪造结果替换拒绝；原始事件前缀保留、修复幂等。
- chat-state `model_skill_integrity`：6 passed。
- `cargo check --locked -p cli`：失败于工作区已有 Shell 存储改动（E0560 / E0599 / E0624，共 4 个编译错误；PersistedInfoLight.history 字段缺失、HistorySnapshot.reseed_event_counter 方法缺失、两处 jsonl 父模块调用 ledger 中私有 append_journal_fact_sync）。本次 sampling-types/chat-state 已通过 check；未顺带修改 Shell。
- `git diff --check`：通过。
- 归档前 `openspec validate --all --strict --no-interactive`：19 passed。

命令使用仓库 Rust 1.93.1、CARGO_BUILD_JOBS=2；测试设置 RUST_MIN_STACK=16777216。新增多技能测试最初遇到 String::as_ref 类型推断错误，改为 as_str 后重跑，未改生产行为。

## 场景对应

Portable/model metadata：`model_skill_pairing_survives_every_prefix_cut`、`native_model_skill_metadata_does_not_split_messages_tool_results`、`reset_prefix_closes_over_model_skill_fact_and_all_results`。

Runtime Chat 恢复：`learned_chat_covers_reasoning_field_on_both_runtime_facts`、`chat_runtime_live_suffix_requires_learned_mode_and_never_borrows_reasoning`、`chat_runtime_only_reasoning_recovery_is_acknowledged_once`。

Responses thinking 收件：`responses_agent_receipt_is_attributed_context_without_synthetic_call`、`responses_agent_metadata_and_body_survive_hostile_text_verbatim`。

完整性证据/缺失/边界：`model_skill_integrity` 集成测试与 `model_skill_batch_preserves_success_through_request_and_timeline_replay`。

## 限制

未向线上 provider 发送用户会话或执行付费请求；验证的是请求 wire、恢复状态和持久化不变量。未安装或替换用户运行中的 grow。历史上已被旧版本提交的错误修复不会自动回滚；原始 Timeline 仍保留事实，本次保留其可验证性。工作区已有其他改动，本次未提交、回退或清理它们。

## 归档结果

`openspec archive fix-runtime-projection-integrity --yes` 完成，合入 4 个修改 requirement。归档后 strict 全量 18 passed；归档校验 681 passed。归档操作自身任务在操作后勾选，首次 archived 校验仅因此失败，勾选后全量通过。两次 CLI check 均因上述 4 个相同 Shell 错误失败。最终测试累计 903 passed、3 ignored；actor 增强断言另行定向重跑通过。

清理限于本次 sampling-types/chat-state 包产物，未删除整个 target 或无关包缓存。

## 单独提交范围复核

以上 903 项与 CLI 记录来自当时完整工作区。本次提交另从 Git index 导出独立快照验证，排除其他 Timeline/Pager/存储改动及此前单独归档的 Responses 用户技能修复。后者与其专用对照测试仍留工作区，不纳入这三项提交。独立快照结果另记于提交前验证。Shell 编译修复依赖尚未提交的存储新文件；不能仅将其几行作为源码提交而不带入整份重构，因此不混入本提交。

暂存快照使用 `cargo test --locked -p sampling-types -p chat-state --lib --test runtime_projection --test model_skill_integrity -- --test-threads=4`：chat-state 570 passed / 2 ignored、sampling-types 312 passed、两组 integration 6 + 8 passed，总计 896 passed / 2 ignored。独立快照 strict 规范校验 16 passed；暂存差异独立只读复核及 `git diff --cached --check` 均通过。与此前混合工作区数量的差异来自明确排除的其他未提交改动，未删除本次失败场景。
