## Why

本机协调询问的逻辑身份与 durable 重放边界未覆盖完整会话身份及接收端恢复：runtime 以 peer 与 inquiry ID 去重，source session 重启可能改变 incarnation；target actor 每次 enqueue 都写 receipt 并重新执行，runtime 重启或缓存过期后同 ID 可再次采样。source Timeline 也未将首次启动绑定到 target/question，响应和取消回执的 ID 未始终核对，可能导致重试改变询问或串扰其他询问。

## What Changes

- 将询问逻辑身份统一为 `(source_session_id, inquiry_id)`；target session 由作用域隐含。peer/incarnation 继续用于握手、租约和审计归因，不作为询问身份。
- inquiry ID 仅接受 runtime 生成的小写连字符 UUIDv7 标准文本；其他文本别名不得形成新的去重身份。
- 由 target Timeline 对重复询问提供 durable 去重：相同身份及 payload 复用终态，open receipt 拒绝再次执行；恢复后已有 open receipt 维持 `TargetRestarted` 终态。
- source 持久化首次启动的 target/question 绑定；完成结果复用，未完成查询返回 `SourceUnavailable`，不自动重新执行。相同 pending ID 只允许同目标同问题重试。
- 校验进度、结果与取消响应的 inquiry ID；不匹配时返回结构化失败，不重发。预取消在 dispatch 前生效，remote terminal cancel 返回 false；跨 source/target 的取消不得影响其他询问。

## Capabilities

### Modified Capabilities

- `local-coordination`：补足询问稳定身份、durable 重放、source 恢复绑定及响应/取消关联约束。

## Impact

影响协调 runtime、source/target session Timeline 接纳与恢复、询问状态/取消 API 以及 UI correlation。保留既有授权关系、foreground 隔离和 Sideband 执行边界。
