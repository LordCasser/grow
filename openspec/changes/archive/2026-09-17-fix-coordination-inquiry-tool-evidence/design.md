## Context

显式 `ask_parent` / `ask_subagent` 经 coordinator 验证直属关系，由 `subagent_coordinator/interaction.rs::ask` 持久记录发送事实并向目标发送 RunCoordinationInquiry。peer 询问进入同一 SessionActor 的 FIFO。ChatState 原子物化 Surface 与 input_ref；当前在冻结后按尾部消息类型删除工具交换，随后交给 InfoRequest Sideband。

自动工具权限判断经共享 permission handle 与 `actor/turn/sampling.rs::wire_permission_auto_llm_classifier` 桥接进入 PermissionJudgment，不能将界面中的询问或自然语言回答作为权限授予事实。协调回答原本不进入目标主 Surface，属于既有隔离契约。

## Goals / Non-Goals

修复冻结输入的证据丢失，覆盖父子与 peer 共用入口。保留既有队列、取消、权限检查、单次回答、审计与恢复边界。不把 Sideband 问答注入主任务，不改权限模式，不新增 pending 工具状态聚合。

## Decisions

用 `project_portable_history(&materialized.surface)` 替换 strip reasoning 和裁尾循环。该既有投影保留可唯一配对的工具交换及附件，过滤未配对协议与原生 continuation/reasoning；与仅保留最后一组的补丁相比，也能处理部分完成批次和有调用的 Assistant 正文。

测试先经过真实 handle_coordination_inquiry 捕获三种 provider 请求并在旧实现下验证失败，再改生产组装。用完成的 ask_parent 后接其他工具结果与待完成批次复现连续尾部丢失，断言调用/结果配对、内容保留、没有工具可执行、没有改变主 Surface。冻结后到达的结果不属于本次快照。

## Risks / Trade-offs

- 输入保留更多实际结果 → 继续遵守现有 Sideband/model 预算，不能以删除最新执行事实掩盖大小限制。
- 正确证据不能保证模型回答正确 → 回归验证实际请求内容，不把固定 mock 回答当作语义准确性证明。
- Atlas 对该 include 模块的局部符号未解析 → 按真实源码、调用入口和 provider 回归核对；不以空结果推断没有调用方。

## Joint architecture review

2026-09-17 应用户要求，与并行任务「修复 btw sideband 上下文获取」（`01a0ae83-2ac6-79a1-9e81-09c4f90d1184`）双向核对。同类缺陷来自各入口把协议配对修复与内容裁减混在一起，并非原子快照取错会话。保持以下既有层次，不新增框架或 Snapshot 包装实体：

1. ChatState 只在一次物化中冻结真实 Surface、身份与 input_ref/revision，不为消费者偷偷删除事实。
2. sampling-types 的既有 portable history 投影负责独立请求的中性工具配对与 continuation 清理。`/btw`、recap、协调询问复用它；明确需要可见 reasoning 时使用已有 with_reasoning 变体。投影不能伪造缺失结果，也不代表未完成调用未发生。
3. 预算与来源选择属于消费者策略：显示性摘要可以有明确预算裁减；会替换主 Surface 的 compaction 必须以完整来源边界缩小实际选区，attempt 的 selected IDs 与成功 replacement target 一致。默认 verbatim 压缩保留已有完整源。
4. Sideband 管理生命周期、预算审计、取消与结果，不统一重写所有输入。PermissionJudgment 有专门的授权 policy 与真实用户输入来源，不能复用含模型/工具输出的完整问答输入作为授权依据。

并行 change `fix-compaction-budget-recovery` 负责压缩、recap、相关 helper 和 backlog；本 change 只负责 coordination 的询问输入及其测试/说明。memory flush 的 simplified 证据损失另记 backlog，不混入本次修复。
