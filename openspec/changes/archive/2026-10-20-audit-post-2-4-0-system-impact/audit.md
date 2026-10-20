# v2.4.0 之后的系统影响与边界审计

## 结论

这轮更新显著加强了持久化确认、失败停止、恢复身份和 UI 投影一致性，但主要风险仍集中在跨 owner、跨阶段的状态交接。**本轮新确认 3 个缺陷：2 个 P1 挂起边界问题、1 个 P2 Goal/Plan 持久快照问题。** 它们均有可执行探针；“新增发现”不等同于“2.4.1 新引入”。

三个问题在 v2.4.0 的相关路径已存在。v2.4.0 之后的修复覆盖了部分相邻路径，但没有关闭这些组合窗口；刚完成的节点挂起修复也未覆盖树级冻结与节点挂起事件之间的窗口。本报告不宣称全仓无其他问题，也不把既有发布绿灯当作这些场景已验证。

## 版本范围

| 层次 | 固定身份 | 范围 |
| --- | --- | --- |
| 基线 | v2.4.0 / `cd5d88ec` | 已包含树挂起和保留 Plan 的基础机制 |
| 已发布版本 | v2.4.1 / `5fcbd12b` | 相对基线 16 个提交、339 个文件，+13,201 / -1,179 行 |
| 当前提交 | HEAD / `02432cb6` | 相对 v2.4.1 再有 3 个提交、73 个文件，+2,501 / -254 行 |
| 当前工作区 | 本会话未提交的挂起修复 | 完成通知复用、人工等待重试保全、question worker 清理顺序；37 项定向回归见前一变更 |

从 v2.4.0 到 HEAD 共 19 个提交、394 个不同文件，+15,548 / -1,279 行。分段文件有重叠；统计包含测试、规范和归档，不能视为 394 个生产文件都经过同等深度审查。正在进行的 MCP elicitation change 不属于已实现的版本差异。

## F1 · P1 · 树级冻结成功但挂起意图尚未持久化时，重试换 owner 后持续失败

**触发：** coordinator 接纳 `FreezeAdmission(op-A)`，随后 ACK 丢失，或子节点初始化等待/核对在 root 写入 `SuspendRequested` 前失败。

实际顺序：

1. `runtime_handoff.rs:1003` 先设置 root runtime_suspend。
2. `:1013` 看到 root Timeline 仍 Running，生成新 UUID；`:1023` 向 coordinator 冻结准入。
3. coordinator `coordinator.rs:325-335` 将 root 绑定到 op-A；已有不同 owner 时拒绝。
4. root 直到 `runtime_handoff.rs:1084` 才提交 SuspendRequested。此前失败时 root 持久状态仍 Running。
5. 用户重试重新生成 op-B；coordinator 保留 op-A，返回 `conflicting suspension owner`。后续重试重复该过程。

**证据：** `tree_probe.rs` 驱动真实 `MvpAgent::suspend_session_tree`，先在 coordinator 传输边界注入首次 ACK 丢失，再以 child 仍在初始化且首轮期限到达的普通失败路径独立复验；两次均观察到首轮和重试 UUID 不同、root 持久 phase 仍 Running。独立 `coordinator_probe.rs` 使用真实 coordinator，验证 op-A 接纳、op-B 拒绝、op-A 重试仍接纳。两者共同验证调用方身份生成和真实接收方契约，未把模拟响应声称为完整子进程 E2E。

**影响：** 退出持续未确认；节点级重试代码无法补救，因为失败发生在节点挂起前。不要把它误判为原故障 session 的已证实根因：原 session 已有 SuspendRequested，本次是另一条可达失败路径。

**归属：** v2.4.0 已存在的顺序缺口，后续观察成本优化未改变关键顺序。

**最小修复方向：** 第一次有副作用的 freeze 前建立可重用的挂起 owner，或者由现有 owner 返回并持久对账同一身份；ACK 未确认时不能重新生成身份。不能简单接受不同 op 覆盖 frozen_roots，这会破坏已冻结 child 的归属。

## F2 · P1 · 人工等待在 SuspendRequested 之前结算为 Suspended，随后被误记 OutcomeUnknown

**触发：** 树级 runtime_suspend 已置位，但该节点的 SuspendRequested 尚未写入；权限/计划审批收到回答、运输层关闭，或问题 receiver 关闭，交互 owner 因挂起状态将原 attempt 记为 Suspended 并清 registry。

`runtime_handoff.rs:1003` 和 `subagent/mod.rs:474` 在树/child 准入阶段置位标志。`actor/tool/preparation.rs:54`、`actor/tool/mod.rs:870` 按此标志结算人工交互；工作区修复后的 question worker 也使用该规则。

当前 `suspension.rs::suspension_interactions` 只保留本次 SuspendRequested **之后** 的 Suspended。有效顺序 `Opened → Resolved(Suspended) → SuspendRequested` 被排除，原有 ToolStarted 又使调用不能归类为 NotDispatched，于是 `suspend_node` 写入 OutcomeUnknown ToolResult，重试 checkpoint 丢失等待。

**证据：** `suspension_probe.rs` 使用真实 guard、Timeline 和 suspend_node：先关闭 runtime 准入并 durable resolve 为 Suspended，再建立节点挂起事件；首次 owner drain 超时，已经出现 OutcomeUnknown；同 operation 重试成功 checkpoint，却没有原 AwaitPermission。探针不声称运行了完整 ACP 权限对话，前置顺序由真实树协调代码和交互 owner 代码证明可达。

**影响：** “挂起成功”也可能保存错误续跑位置；恢复不再重建原问题/权限等待。现有 37 项回归覆盖的是挂起事件之后的 guard 清理，对前置窗口没有保证。

**归属：** v2.4.0 的易失 registry 快照也会漏掉已经清理的等待；前一工作区修复从 Timeline 重建后，仍保留了这个盲区。不能称为刚才补齐了所有恢复场景。

**最小修复方向：** 统一准入关闭、交互 Suspended 归属与持久操作边界，让这一段真实等待可归属到原 operation。不能无条件复活所有历史 Suspended；仍须隔离旧恢复周期、已回答/拒绝的 attempt，以及真实 ToolResult。修复需覆盖 root 和被提前 freeze 的 child。

## F3 · P2 · 自动 Goal 激活/重激活仍丢失持久保留计划与提交计数

**触发：** 用户保留计划，合法离开 Plan 到 Normal；之后自动 Goal 激活。该行为已接纳并持久确认，但在下一次完整快照或正常挂起前异常退出。

`goal.rs:30` 的 `selected_behavior_preserving_completed_plan` 从空白 BehaviorSnapshot 开始，只在 completed_plan 存在时补回完成引用和计数，始终未复制 retained_plan。`:1208` 的 Goal 激活及 `:1338/:1345` 的 Goal 定义转换复用此 helper。

持久化 helper 直接写入传入快照；随后内存 `BehaviorCoordinator::select_behavior` 清理 Plan runtime，却继续保留独立候选和计数。因此运行中的 UI/query 与冷恢复状态分歧。

**证据：** `goal_probe.rs` 调用真实 initialize_goal_runtime：

```text
live submission=1, durable submission=0
live retained=true, durable retained=false
```

**影响：** 冷恢复丢失候选发现入口、提交身份回退。正常挂起会重新捕获完整内存快照，可缩短窗口；不是每次退出都会丢失。本轮没有证明因此能够绕过审批，不作安全绕过结论。

**归属：** v2.4.0 已使用空白 selected 快照。完成计划功能新增了只保护 completed_plan 的 helper；之后手动 Behavior 切换修复没有同步覆盖自动 Goal 路径，是同类缺陷修复不完整。

**最小修复方向：** 与手动选择一样，从真实 coordinator 快照派生合法目标状态，保留独立候选和单调提交计数；不要继续逐字段针对某一种 Plan 状态打补丁。覆盖首次 Goal、重激活、无 completed_plan 但有 retained_plan、异常恢复和后续新提交。

## 整体影响矩阵

| 系统面 | 更新与正面影响 | 扩散到的链路 / 当前剩余边界 |
| --- | --- | --- |
| 挂起、退出、恢复 | 关闭辅助准入、排空已接纳 owner；复用命令证据，增加阶段日志；Pager 可重试退出 | root coordinator → child freeze → node checkpoint → writer retirement；F1/F2 表明正确性不能只以节点 checkpoint 测试判断 |
| Plan、Goal、Agent 切换 | 完成报告有不可变引用和详情页；手动切换保留候选；Agent 重建重新绑定 TODO runtime | Behavior 内存 → Control → restore → Pager；F3 是自动路径遗漏，增加独立字段后要检查所有快照构造入口 |
| 采样和工具执行失败 | 管线错误沿 fatal turn boundary 停止，不让 completion recovery 继续采样；保存原错误因果 | ToolStarted/结果/ToolCompleted → Step → Turn；失败应停止依赖执行，业务工具错误仍沿正常结果处理。静态核对未确认额外新缺陷 |
| 图片恢复和对话投影 | typed operation、child 路由、reconnect 去重、durable sideband 复用、去掉内部占位文本 | Shell → typed Grow event → root/child live/history → scrollback → provider request；旧审计 F2/F3 已有修复。静态抽查，不代表重跑所有 provider/重连组合 |
| Skill 加载与权限 | 增加受约束的资源渐进读取、身份复核、大小/UTF-8/取消边界；Timeline 验证 resource 参数与接纳调用一致 | discovery → preflight → 文件权限 → bounded read → durable body → replay；检查了消费方，未确认新增绕过，不将普通源码阅读等同安全穷举 |
| Bash 与本地命令 | 完整快照终止符、allexport/导出变量隔离，减少 shell 环境污染 | 命令执行 → fd 快照 → 后续命令；Windows 平台字段修正和 Unix shell 改动分别看待。未重新执行各 shell/平台矩阵 |
| Resident/config 生命周期 | watch 从仅注册变为 resident 持有、释放；事件送达时再次核对归属 | session map → watch registration → notify callback；正常 update 复用同一 registration，未确认重复 owner 的可达新增缺陷 |
| 历史、导出、性能 | child 历史避免冗余复制/校验；轨迹导出补资源收尾；投影恢复保留事实身份 | raw Timeline、派生 Surface、UI cache 仍是不同层；合成测量不能推断真实长会话或整树期限表现 |
| 测试与发布 | exact runner 分阶段预算/收尾；same-SHA 发布门禁、平台 warning 门禁；嵌入构建身份传递 | 构建通过不能验证跨阶段业务顺序。公开 v2.4.1 产物仍是 5fcbd12，不含后续 HEAD 或工作区修复 |

## 验证强度与空白

- 既有发布记录报告 v2.4.1 的 16,467 个 library case、30 个明确选择的集成/PTY case 和 20 个 workflow job 成功。本轮只读取这些已归档证据，未重新访问或执行 hosted 发布流水线。
- 本轮执行 4 个定向审计探针，全部观察到预期缺陷/接收方约束；“passed”表示反例复现成功，不是这些产品行为正确。
- 本轮未重跑全仓测试、真实 provider、跨平台终端、Windows/OHOS 物理设备、长会话性能或完整崩溃重启矩阵。Atlas 返回的局部定位不能证明不存在其他调用方；Goal helper 的 scoped search 无匹配，最终以源码核对。
- 不修改已发布 tag/产物，不触碰原故障会话。测试临时接入点已删除，复现源码保留在本审计目录。

## 后续处理次序

1. 先处理 F1/F2 的挂起 owner 与交互边界；它们共享前置准入窗口，需一起设计时序，但分别保留反例。
2. 独立修复 F3 的自动 Goal 快照派生，避免把它混入挂起重构。
3. 后续回归以阶段组合为单位：freeze ACK 未确认、持久意图前交互结算、checkpoint 前失败、同身份重试、冷恢复、重新打开等待并回答。保留已有单模块测试。

架构不需要为此新增一套协调框架；现有 Timeline、Control 和 coordinator owner 足以承载修复。需要补齐的是这些 owner 交接时的身份与事实边界。
