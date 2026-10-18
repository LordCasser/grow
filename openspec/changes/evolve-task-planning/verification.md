# Verification

## Scope

- 日期：2026-10-18。
- 文档核对 HEAD：`dd034d2dae82c03bd9715a3f19d1c17b49402a80`。
- 产品代码核对基线：`0a0dd07cd3aa35d39365ae971df8f9b316185c9d`，后续相关提交为 OpenSpec 文档。
- 仅新增 `openspec/changes/evolve-task-planning/`。未实现产品代码、未修改主规范、未归档；按用户后续要求，仅提交本 change 文档。
- 开始编写前工作区已有未跟踪的 `fix-null-chat-usage-details/` 与 `unify-skill-loading/` change；它们不属于本次范围。验证期间观察到前者被并行工作归档，另有 sampler/sampling-types、开发指南和 model-sampling 主规范变化。本次未写入这些路径，也未撤销或修复这些并行变化。已有 `suspend-session-tree`、`retain-plan-for-later-review` 等不修改。

## Evidence review

已按源码核对 TODO 的 Resources 写入与异步保存、task owner 与直接父级、nested `surface_completion=false`、事件 waiter、parent message Received/Consumed、child runner 终态、Goal 内外层 idle 准入、turn-end 假完成 UI 及 compaction reminder。入口见 [design.md](design.md) 的 Current implementation 表；当前能力与未来需求分别标识。

主规范核对范围：session-timeline 的持久权威、消息、结果/收尾/清理与恢复；behavior-goal 的执行权威、预算、revision 与全目标审计；tool-authorization 的直接关系与权限；client-surfaces 的 replay/恢复/观察；context-compaction 的发布与历史来源。冻结交互对照未实现的 `suspend-session-tree`，不将其当作当前已有能力。

## Review and static checks

按项目要求由 Luna 进行一次只读对抗性审查及修正后复核，以下六项已闭合；复核未发现阻断性文档缺口：

| 审查发现 | 修正与验收落点 |
| --- | --- |
| Consumed 未 Handled 的普通退出可能被误读为可恢复 | ST-04 只允许合法冻结继续；ST-15 对终态记未应用，未知实例不提前制造终态 |
| 已 Received 消息不能由发送方单方 supersede | 保持原消息 identity/payload，ST-14 按消费边界和已消费来源集合确认覆盖 |
| 直接父级交付缺少 root Goal 验收联系 | ST-16/BG-08 使用逐级局部交付摘要，不复制 grandchild 模型回执，不授予 Task child Goal authority |
| 多次 yield 可能每次重新取得预算/max-turns | TP-31 明确原 execution owner 跨唤醒和恢复累计 |
| 父 TODO 取消与执行取消、解除义务混淆 | design 区分三种操作，TP-32 验证通知未达时在途工作仍可能继续 |
| 暂停/权限等待可能被更新唤醒绕过 | BG-09 明确只保留责任，执行仍由原 owner/control 准入 |

静态验证结果：

| 检查 | 结果 |
| --- | --- |
| `openspec validate evolve-task-planning --strict --no-interactive` | 通过 |
| `openspec validate --all --strict --no-interactive` | 19 passed，0 failed（当前工作区，含并行工作当时状态） |
| `openspec status --change evolve-task-planning` | 4/4 planning artifacts complete，仅表示 proposal/specs/design/tasks 齐全 |
| Python 静态映射/文本检查 | 12 个文件、6 个 capability delta、23 个 Requirement、69 个唯一 Scenario；与 acceptance.md 的 69 行一一对应，均保留未执行状态；本地 Markdown 链接存在，无尾随空白 |
| `git diff --check` | 通过；另用 Python 覆盖尚未跟踪的新 change 文件 |
| 范围核对 | 本次所有写入均位于新 change；与开始时 OpenSpec 哈希快照对比发现的其他变化仅记录为并行工作，不宣称整个工作区未变 |

OpenSpec 结构校验和文档审查不证明运行行为已经实现。仅 tasks 第 1 节勾选，其余实施、运行验证与归档全部未勾选。

## Not run

[acceptance.md](acceptance.md) 中所有运行场景尚未执行。未运行 Rust 编译、单元/集成/端到端测试、真实冻结恢复或产品 UI 验证；本阶段没有代码实现可验证。未运行 archive。
