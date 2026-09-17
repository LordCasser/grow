## Evidence and scope
已有 context-compaction 只明确保护未替换 tail，未约束降级输入与 target 一致；本次补足契约。旧摘要 Sideband 的 max_output_tokens=None，sampler 会填主模型 output_limit；局部准入不含这部分预算。旧固定 32768 余量对小窗口可归零，随后 simplified 删除结果。Size/still-over-window 的 sticky 状态跨 turn 阻断自动压缩。

## Decisions
1. 摘要输出显式上限为窗口八分之一、最多 32768 tokens，且不超过显式模型上限；输入另留窗口 5% 估算余量。Sideband manifest 与三个 backend 的实际请求共用上限；不可依赖 provider 默认。
2. 先按现有 16% tail/5000 最小来源选区，再以完整请求预算缩小选区的旧历史前缀。边界不得拆开 reasoning/assistant/tool-result 响应组，未选中的消息身份不变。所有重试都只引用原冻结快照。
3. 明确 overflow 最多两次缩小完整选区（每次进一步降低来源预算），不再截断单条正文或删工具结果。普通 transient/degenerate 重试保持同一输入；不能选出足量完整来源则无替换地失败。non-verbatim 模式使用已有 portable 投影并保留执行证据。
4. 每次 Sideband attempt 记录实际选中的 Surface IDs；成功 summary.target 与该次输入一致。基础 System 单独作为 context。
5. Size 和 post-commit-over-window 仅 turn-scoped suppression，下一次真实 turn 可再次尝试；仍保留有界重试、fatal durable-write fail-closed 与授权/账号抑制。错误通知不暗示 session 已作废。
6. Recap 使用现有 portable 历史投影替代误删完整尾部的循环，预算裁剪后再次确保工具配对；不混入 recap 输出预算重构。

## Risks and verification
本地估算无法保证 provider tokenizer 完全一致，因此需注入 provider overflow，验证选区收缩与 target 同步。不可分割超大工具交换仍可安全失败，不承诺任意输入都能压缩。测试需覆盖三 backend 请求上限、完整工具输入、降级身份、失败后下一 turn 恢复、同 turn 有界失败、异步发布与取消。保持原始账本、近期 tail 及已有控制恢复顺序。原用户事件暂无原始证据。

## 联合核对

与 `fix-coordination-inquiry-tool-evidence` 对齐：无需新的 Sideband Snapshot/Builder 实体，冻结身份继续由 ChatState 拥有；需要工具历史协议规范化的消费者直接复用现有 sampling-types projector。压缩的选区缩减与 target 一致性属于有持久替换语义的消费者职责，不能转嫁给通用 projector。默认 verbatim 保留完整合法选区来源；non-verbatim 与 Recap 使用 portable。PermissionJudgment 的授权来源另有严格契约，完整问答上下文不能成为用户授权证据。memory flush 的 lossy 摘要输入登记 backlog，保留独立设计边界。
