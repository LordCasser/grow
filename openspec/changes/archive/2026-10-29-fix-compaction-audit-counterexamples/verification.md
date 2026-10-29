# 验证记录（2026-10-29）

## 环境与复现
当前 macOS arm64 工作区，Rust 1.93.1；`--locked`，`CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=4 RUST_MIN_STACK=16777216`。未改写用户生产 session，未安装或重启 Grow。

修复前六个探针全部失败，分别为两个取消探针及预算、清理、四种错误 provider stop、排队空转探针。原始证据保存在相邻 audit-all-compaction-paths change。修复后移入正式测试，保留不变量而非断言实现细节。

## 回归过程

| 检查 | 实际结果 |
| --- | --- |
| Shell 首次全库 | 4376 passed、2 failed、9 ignored（154.38s） |
| ChatState 首次全库 | 584 passed、2 failed、3 ignored（21.31s） |
| common compaction 首次全库 | 58 passed、0 failed |
| 修正后的 ChatState compaction 相关回归 | 180 passed、0 failed |
| 修正后的 common code_compaction 相关回归 | 35 passed、0 failed（包含新增 formatter 测试） |
| 修正后的 Shell compaction 相关回归 | 108 passed、1 failed；剩余为 Goal fixture 对全局 provider closed 的错误断言 |

首次编译发现新增 match guard 使用实验语法、测试对未实现 PartialEq 的 ConversationItem 直接比较；改为稳定 if-let 和序列化后的值比较后编译通过。没有改动生产数据表示来迁就测试。

全库失败收尾：formatter 的两个既有显式 Markdown scratchpad 形式已补回，同时保留自然语言正文反例；typed 超窗内部文案改为已有分类器识别的 canonical code；取消已生效时 Goal fixture 不再要求 finished worker 常驻 slot。该 Goal 已成为 BudgetLimited，不能要求全局 provider window 仍是 Active Goal 的 closed 状态，而应验证原 Goal 无法再准入。mock 在 terminal 前阻塞，usage 已先到达，保留原来的 148025 已知总用量，不能将取消当作零账单。

上述收尾未删除或忽略任何失败测试，未放宽时间限制。最终结果在下方补记；不将定向复验称为另一轮全库。

## 场景映射

- 未提升取消：`ordinary_cancel_signals_unpromoted_generation`；阻塞 provider 的 `compaction_regression_ordinary_cancel_discards_released_background_summary` 强制经过真实 publication gate，验证无 Completed/摘要安装并排空 owner。
- 手动排队：`compaction_regression_pending_manual_compact_does_not_spin_automatic_recovery` 验证 Step 成对闭合、零普通请求、保留输入与手动指令，再由真实 idle arbiter 完成一次摘要。
- 来源预算：`summary_budget_ignores_reasoning_that_will_not_be_sent` 覆盖 Messages、保留 reasoning 的 verbatim、portable 三种投影；ChatState fit 用例继续验证完整工具往返、minimum、identity；所有 backend 的 bounded request 集成用例通过。
- 输出验收：四种 typed 非成功 stop 都不替换 Surface；未知/缺失终态分类单测和真实 Responses EOF mock 覆盖完成证据缺失；正常三种成功终态继续通过。
- 清理：自然语言正文引用 closer、Analysis of… 开头、既有括号/同行 Markdown scratchpad 全部覆盖。
- 周边：Shell 全库执行了 Goal、rewind、model switch、image fallback、Plan/control、持久化恢复、Sideband settlement 相关测试；本轮未修改 Pager，前序 Pager 终态回归记录仍独立保留。

## 限制
真实 tokenizer、线上 provider 故障、跨平台 CLI 发布不是本次本地测试的证明范围。没有合法完整摘要来源或一次有界摘要后仍超窗时，仍明确失败并保留历史。审计未证实的组合缺口独立登记在 backlog，没有通过引入重试墙或另一个状态机掩盖。

## 最终复验

最后的 Goal budget 回归已通过：1 passed、0 failed（0.67s）。Shell 相关 109 项的 108 项已在前一轮通过，最后只改 fixture 的生命周期/计费断言；当前生产代码没有再变。ChatState 两条既有 formatter 失败和新超窗分类测试也均在定向回归通过。已执行的全库非 ignored 用例没有留下未解决失败；这不表示又完整运行了一遍全库。

原始红测及首轮编译问题、测试断言纠正过程在本记录保留。普通取消不会吞掉已观测费用：mock 的 terminal barrier 位于 usage 之后，保留 148025 的总量断言；仅原 Goal 的后续 attempt 被拒绝，BudgetLimited 不意味着阻止所有新的普通用户请求。

非测试构建 `cargo build --locked -p shell -p chat-state -p compaction --lib` 通过（1m40s）。`cargo fmt --all -- --check` 和 `git diff --check` 通过。归档前严格全量 OpenSpec 校验 18 passed、0 failed。

验证完成后 `cargo clean` 删除 11,620 个文件、4.5 GiB 构建产物；没有保留 Cargo 编译缓存。

归档后：严格全量校验 `Totals: 16 passed, 0 failed (16 items)`；archive 校验 `Totals: 718 passed, 0 failed (718 items)`。确认 target 目录已移除。
