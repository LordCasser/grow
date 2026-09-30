## ADDED Requirements

### Requirement: Offline transcripts preserve interrupted turn ownership

Export 和 replay SHALL 使用同一离线展示投影，按已记录 prompt identity 处理 turn 边界。取消、失败及其他非正常 stop reason SHALL 可见；未记录原因时不得猜测。Turn terminal 或后续用户输入 SHALL NOT 充当任何 pending 工具的成功结果；无工具 terminal 的行 SHALL 保持截至快照未完成，晚到真实工具 terminal SHALL 更新原行。

#### Scenario: User cancels and continues
- **WHEN** 一个 turn 在正文或工具途中取消，随后用户发起下一 turn
- **THEN** 显示已允许的历史正文、取消事实与新输入；两 turn 的消息分开，未闭合工具不被标为成功。

#### Scenario: Earlier terminal arrives during another turn
- **WHEN** 前一 prompt 的 terminal 到达时已有不同 prompt 的正文
- **THEN** terminal 只描述所属 prompt，不收束或拆分当前 prompt 的消息。

#### Scenario: Background work finishes after a turn
- **WHEN** TaskBackgrounded 后跨 turn 出现 TaskCompleted
- **THEN** 保留一条 started 和一条对应 terminal 展示，不将 started 改写成第二条 terminal。

#### Scenario: Persisted user echo is repeated during its response
- **WHEN** 同一 message identity 的用户 echo 在正文中重复到达
- **THEN** 不重复用户行，也不切开所属回复。

#### Scenario: Passive history requires an existing fact projection
- **WHEN** 已校验 Timeline 包含缺失展示的 completed Hook、direct child lifecycle 或 parent/agent reply receipt，或者已保存的 UiNotice 包含通信 receipt 正文
- **THEN** 复用相应纯展示转换保留事实和稳定身份，不运行 handler；补回事实缺乏原始展示时间时明确使用估计顺序。

#### Scenario: A retained message receipt has no readable body
- **WHEN** receipt 的 immutable 正文 artifact 已缺失或校验不通过
- **THEN** 与 live reconnect 相同，保留 receipt identity 和 Message unavailable 提示，不重投递消息、不虚构正文。

### Requirement: Offline snapshot frontiers survive normal append

Transcript reader SHALL 在解析前固定必要 ledger 的文件身份和长度，目录树发现与正文消费 SHALL 复用该捕获。预算 SHALL 在物化前约束 ledger 字节并累计必要引用与记录。正常追加 SHALL 不延长读取；捕获范围中的完整损坏行、截断或身份冲突 SHALL 明确失败。

#### Scenario: Writer appends after capture
- **WHEN** writer 在捕获后补齐半行或追加新的消息、turn terminal 或 child spawn
- **THEN** 该次输出仅消费已捕获的完整行，之后的记录不进入正文或目录树，也不因单纯追加失败。

#### Scenario: Committed prefix is invalid
- **WHEN** 捕获前缀含完整坏行、外部 session 身份或读取中发生截断
- **THEN** reader 返回定位明确的错误，不导出空成功或混入其他 session。

#### Scenario: Canonical response replaces a preview anchor
- **WHEN** 已准入响应取代同 sampling attempt 的展示锚点
- **THEN** 保留锚点记录的 prompt identity 和原始时间，不以本次读取时间代替；未准入 attempt 的正文不进入历史。

### Requirement: Replay semantic boundaries take precedence over simulated text

到期的依赖事件 SHALL 收束此前模拟正文并立即按既有逻辑顺序交付；模拟动画 SHALL NOT 推迟用户打断、下一输入或工具边界。暂停 SHALL 同时冻结事件与模拟正文。终端粘贴 SHALL 被视为整体输入并忽略，不触发播放器快捷键。

#### Scenario: Cancellation arrives during reveal
- **WHEN** 文本仍在模拟揭示，而取消 terminal 或新用户输入已到期
- **THEN** 先收束已记录正文，再在本 tick 交付到期边界；下一消息不继承前一 reveal。

#### Scenario: Paste contains playback keys
- **WHEN** 粘贴的正文含 q、空格、加减号或 slash command
- **THEN** 不退出、不调速、不暂停、不执行命令；独立键盘快捷键仍可浏览或退出。
