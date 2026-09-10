## 1. 架构与契约

- [x] 1.1 核对三协议、sampler、session 接纳、通知及用量路径，交付 proposal/design/delta/verification；通过本 change strict 校验。

## 2. 失败事实与执行策略

- [x] 2.1 在修改每个入口前复核实际调用方与消费能力，记录到本 change；覆盖主/子 agent、buffered auxiliary、headless partial 和外部 ACP，未知能力保守停止。
- [x] 2.2 三协议输出 typed 不完整流、协议冲突和完整非法生成事实，保留 EOF/DONE/timeout 等终止原因；用 parser 测试覆盖完整性、冲突优先级、usage tail 及原始错误来源。
- [x] 2.3 内部 attempt 失败结果不再通过 SamplingErrorInfo 文本往返；用类型投影测试验证诊断不改变重试语义，API veto 和取消原因仍保留。
- [x] 2.4 以无 I/O 判定函数收敛恢复条件；mock HTTP 验证无输出断流后成功、重复断流到上限、确定性错误仅一次请求，已有输出暂保持严格保护。

## 3. 结算与准入

- [x] 3.1 用稳定 attempt 归属接入普通账本、Goal 及子任务输出预算，移除接纳/终端事件的重复消费累计；集成测试验证失败后成功两笔各计一次，scope=None 也生效。
- [x] 3.2 验证部分结算失败、ACK 丢失、取消和 parent fold 不会丢失或重复消费；故障注入断言在确认前没有新 provider poll，未知用量保留原预算约束。
- [x] 3.3 每次准入重算子任务剩余输出许可；mock 请求断言第二次 token 上限已缩小，已耗尽或未知消费时不派发。
- [x] 3.4 让 sampler 与 session 修复重提交共享总次数和绝对期限，doom 等分类额度只收紧；交替故障、显式关闭、backoff 取消和过期 owner 测试断言实际 HTTP 请求总数。

## 4. 预览与响应接纳

- [x] 4.1 沿 sampler、shell 通知、合并/replay buffer 传递 attempt 归属及 Begin/Discard/Accepted 边界；乱序、迟到 delta 和 debounce 测试验证不会跨 attempt 合并。
- [x] 4.2 Pager 支持废弃候选；headless/外部客户端按交付能力约束恢复，最严格消费者规则有效；真实 reducer 测试覆盖 ResponseStarted、文本、reasoning、signature、工具参数及多消费者、断线丢失边界与复用子任务视图。
- [x] 4.3 durable admission 确认后才发布 Accepted 和允许工具；集成测试覆盖拒收 sibling、确认丢失及 cancel/terminal 竞争，断言没有重复接纳或工具执行。
- [x] 4.4 在完整链路验证后开放可废弃输出的重采样，并将非法参数/doom 旁路并入同一策略；三协议 mock 与 shell/Pager/headless 联动测试验证旧预览不串入新答案。
- [x] 4.5 PersistenceActor 暂存带 attempt 归属的 ACP preview，按 Accepted/Discarded/停止边界提交或丢弃，并用临时 storage 回归验证 discard、未接纳停止和 untagged interleaving 的 replay 结果。

## 5. 验证与归档

- [x] 5.1 按 verification.md 矩阵完成故障注入，并记录事故会话取证若可得；对未取得的线上证据保留限制，不以模拟数据证明真实上游根因。
- [x] 5.2 更新 docs 中采样边界、诊断与恢复配置说明并链接契约；运行受影响 sampler/sampling-types/shell/chat-state/pager 的库与集成测试，以及 `cargo check --locked -p cli`。
- [x] 5.3 场景和测试记录逐项对齐后运行 `openspec validate --all --strict --no-interactive`，再归档本 change；归档后执行全量与 archive 校验。只在实现与验证完成后归档。
