## Context

`prepare_compaction` 按窗口的 16% 设置保留预算、最小摘要源 5,000 tokens，调用纯函数 `plan_compaction_range`。函数先找覆盖预算的 prompt suffix；若无法替换完整旧回合，再细分响应组。现有 fallback 错用 `last_prompt` 而非前一步选定的 `tail_start`。

实际 Timeline 重放有 260 个 Surface 项且身份数量相等，估算 167,770 tokens；带 prompt index 的位置为 7 和 180。预算 40,960、32,000 时旧逻辑都无计划，16,000 时可选范围 7..179。Goal 投影保持项数，不是身份错位。

`run_compact` 已发送失败通知并用 `mark_control_terminal_published` 标记错误；`extensions::memory::handle_compact` 又将该错误格式化成字符串，Pager 无法识别标记。

## Goals / Non-Goals

完成本会话暴露的选区及终态反馈闭环。不调整 tokenizer、保留比例、摘要阈值、prompt 标记的含义、Goal 生命周期、provider 或持久化结构。

## Decisions

1. fallback 从 `tail_start + 1` 开始，保留该 prompt，沿已有响应组边界与 suffix token 检查选择范围。后续 prompt 及工具结果自然留在 suffix；无需降低保留预算或引入新的选区策略。
2. RPC 仅把 oneshot 接收失败映射为 transport/internal error；actor 返回的 ACP Result 用 `?` 原样传播。沿用现有结构化终态标记，避免客户端按报错文本猜测。
3. 测试覆盖跨 prompt 长片段、reasoning/tool 配对、确实无足够来源的拒绝、RPC 中 code/data 保真、Pager 接收失败通知后终止等待。用户 Timeline 只读探针验证实际范围，不提交原始会话内容。

## Risks / Trade-offs

- 选区可能包含较早片段内部的响应组 → 继续使用现有组边界、Surface identity 和前台提交验证，测试完整调用/结果顺序。
- provider 摘要仍可能失败 → 本改动只解决本地选区和错误反馈，不承诺外部服务成功。
