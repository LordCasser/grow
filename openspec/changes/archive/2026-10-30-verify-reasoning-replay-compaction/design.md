## 方法

以 Timeline 的 sampling evidence chunk 和 source projection 为依据，核对切换前后的完整 wire。补回 reasoning 必须遵循生产 portable 投影的邻接、assistant 可见性和完整工具配对规则；去除新增 reasoning 后必须与原拒绝请求完全一致。

优先使用原端点计数接口；不支持时以最小输出预算的独立请求获取 usage 或明确的上下文超长拒绝。输出预算的诊断改动单独记录，不能拿减小后的预算证明原配置可容纳输入与输出。请求不绑定 Grow actor、不执行工具、不写原会话。

只有当前 provider 的回执可以证明实际输入计数。字符数、旧模型 usage、其他 tokenizer 和估算值均不能冒充该回执。受控变体的修改及计数必须与原失败请求区分。

## 最小修复

SessionActor 在应用闭合 Step 的控制队列前后比较现有模型路由快照的 catalog identity、wire model、backend、base URL 和 query parameters；仅实际路由变化需要交接，effort/输出预算等普通参数变化不追加提示。

复用现有下一 Step 准入门：当前 owner 仍可继续且 Step index 大于零，先 durable append 一个 `ConversationItem::auto_continue`，再启动 Step。提示明确模型切换、保留历史工具结果、只继续原已授权未完成任务，不授予权限、不要求重复工具。若同边界已有异步压缩续接，复用该 user 边界，避免双重提示。没有获准后继 Step、首 Step 的真实新输入、同路由参数变化均不添加。持久化失败沿既有 fatal turn boundary 失败，不派发下一请求。

不加 provider 名单、计数探测请求、校准副账本或调用 ID 改写；校准与真实超窗处理保持当前契约。为隔离兼容问题，线上实验曾改写一对 ID，但该实验手段不进入产品。
