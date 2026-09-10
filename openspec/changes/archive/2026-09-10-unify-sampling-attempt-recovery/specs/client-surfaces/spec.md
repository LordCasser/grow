## ADDED Requirements

### Requirement: Sampling previews are isolated by attempt and delivery capability

采样输出 SHALL 显式声明最终结果交付、可废弃 attempt 预览或不可撤销流的交付能力。可废弃预览 SHALL 在 text、reasoning、工具参数、signature、合并缓冲及终态上保留请求与 attempt 归属，废弃旧 attempt 后才能显示下一 attempt 的内容。不可撤销的响应帧一经外发 SHALL 阻止透明重新采样。多个消费者 SHALL 按实际交付边界采用最严格限制；对可废弃候选，不支持撤回的附加观察者 SHALL 先缓冲到接纳后交付，从而不暴露被废弃的预览。发起不可撤销标准流的客户端 SHALL 保持实时输出及输出后停止恢复的约束。

#### Scenario: Retry after preview output
- **WHEN** 支持 attempt 废弃的 Pager 已展示部分文本和工具参数，随后允许恢复
- **THEN** 旧 attempt 的预览被标为废弃或移除，合并缓冲先处理废弃屏障，新 attempt 独立累积且旧迟到 delta 不污染它。

#### Scenario: Irreversible headless output
- **WHEN** Minimal 的原生终端滚动区、标准 headless 或外部客户端已经收到无法撤销的响应帧
- **THEN** 后续失败终止当前输出，不把新生成内容拼成同一成功响应；只收最终结果模式可依其能力恢复。

#### Scenario: Consumer capability becomes stricter
- **WHEN** 同一请求存在多个消费者或中途接入不支持撤销的消费者
- **THEN** 不假定所有已发布输出可撤销；可废弃候选的未知观察者只在 Accepted 后收到缓冲内容，Discarded 后零候选内容外发，已接纳历史的 load 不重复补发该候选。

#### Scenario: Durable admission is not acknowledged
- **WHEN** 候选流完成但会话接纳尚未确认
- **THEN** 客户端仍将其视为未接纳候选，不发布 Accepted 或可执行工具结果。

#### Scenario: Recovery is denied
- **WHEN** 请求因输出不可撤销、用量不完整、额度耗尽、owner 失效或协议冲突而停止
- **THEN** 终止诊断在既有 attempt evidence 中保留对应停止条件及累计 attempt 数，常规自动恢复保持正常运行活动而不额外弹出警告。

#### Scenario: Reconnect misses the candidate terminal boundary
- **WHEN** Pager 断线期间错过候选的 Accepted 或 Discarded，主会话或复用的子任务视图仍持有未确认预览
- **THEN** 重连丢弃未重新确认的旧预览，只以已接纳内容展示成功历史，不因较新的独立事件 cursor 保留废弃预览；加载失败保留候选归属，再次重连也不能把未确认候选变成已接纳历史。

#### Scenario: Interaction or replay overlaps a sampling candidate
- **WHEN** 候选仍待接纳时到达文件/终端等驱动客户端请求、权限请求或定向历史回放
- **THEN** 原交互与定向路由继续生效，不把请求缓冲到接纳之后或广播给附加观察者；普通交织通知与候选在最终交付时保持原 eventId 顺序。
