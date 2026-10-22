# 对抗性复核

- 核对构造与消费点，而不是直接接受 large_enum_variant 的外层 Box 建议。Timeline 先缩小 Spawned/Checkpoint；ToolPreflight 缩小 Dispatch 和稀疏 hook，保持小结果内联；TimelineBootstrap 的 history 保持内联。独占所有权未改成共享或延迟持久化。
- ACP 请求只在入 mailbox 时分配，出队 move 解包；取消、responder 及发送失败路径不变。Git DTO 的 tag/字段、Timeline、输入清单的 serde 结构不变。Box 的透明编码由整库回归覆盖。
- 恢复中的借用快照使用 `.as_ref().clone()`，避免克隆 Box 后拆箱。输入 StoredBlock 在读取时移出内部 ContentBlock。子会话回应丢失仍保留完整结果，协调器重试仍保留原输出和 deadline。
- Hashline 错误只在出错时分配，公开结果仍是原错误值；不改变错误种类和提示。ParamValidationError 只间接承载可选 bad_value，消费者取回原 Value。
- 18 个类型别名逐项按原类型展开，保留 tuple 次序、Future output、HRTB、Send/Sync 与 cfg。发现过 TaggedFuture 泛型个数错误，已由类型检查捕获并修正，未把失败日志当通过。
- 四个局部 expect 分别覆盖 Axum 拒绝响应、频繁克隆的草稿快照、两个临时 journal 枚举；不为 lint 给这些路径新增分配。其余 Box 仍有一次堆分配成本，主要换取异构消息槽和 future 栈尺寸下降，不宣称 resume 加速。

只读复核另核对 acp-transport、chat-state 与 workspace-types，未发现额外 clone、字段/tag/cfg 改变或断言丢失。所有调整都需最终整库回归与全目标检查通过后才能归档。
