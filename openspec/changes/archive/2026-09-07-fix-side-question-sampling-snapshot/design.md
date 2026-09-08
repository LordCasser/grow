## Context
handle_side_question 先 prepare_chat_completion，再 materialize_timeline，随后 get_sampling_config 设置显式 model；client 只在 model=None 时回退默认模型。

## Decision
与 recap 共用配置准备 helper，保存 model 后消费 config 构造 client。后续不重读路由，不添加锁或扩展旁路框架。

## Validation
复用双 mock endpoint 的受控切换测试，以各自 cfg(test) 一次性 callback 在 client 准备后切换配置；验证实际请求仍在旧 endpoint 使用 old-model，并保持新会话配置。
