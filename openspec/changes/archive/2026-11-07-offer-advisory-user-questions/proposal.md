# Why

用户要求 ask 支持两种形式：有推荐方向时提问后继续；必须由用户决策时等待。默认 Agent 逐次选择，Settings 可持久关闭非阻塞形式、保持原有阻塞形态。另一个用户报告是旧任务结束后迟答 ask 可能卡住；本特性需要从契约上避免把新的用户反馈送回已结束工具调用。

本地 Codex 已核 `RequestUserInputArgs.is_blocking`、UI 及 auto-resolution 字段，但当前本地 handler 仍 await request response，并且按 Plan/Default 决定 flag。不能把那个 flag 直接视为 Grow 模型已能继续；本次按用户要求定义实际模型执行行为。

# What Changes

`ask_user_question` 增加明确的 blocking 选择和每题推荐项。非阻塞形式作为“建议已展示、按临时假设继续”的工具结果立即结束，不占着旧工具等待用户；用户之后改选经现有用户输入接纳进入会话。阻塞形式保留原等待/取消/超时与持久交互政策。Settings 增加持久 `allow_non_blocking`，默认 true，false 强制全部 ask 阻塞。

# Impact

Tools 拥有参数校验与模型结果；Shell 拥有有效用户偏好、展示提交与输入准入；Pager 拥有两种问答展示及用户回复。复用现有持久工具记录、用户输入快照与设置原始写事务，不新建通用 question runtime 或无限后台 reverse-RPC。此特性已经获用户实施授权；只有另项模型池明确要求先批准方案。
