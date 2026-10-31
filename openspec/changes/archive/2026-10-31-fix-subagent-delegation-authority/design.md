## Decisions

复用 selector_identity 作为授权键。路径 selector 的每个段必须是普通名称，禁止空段、`.`、`..` 及绝对路径；插件 bare fallback 可解析，但必须以限定 ID 授权。按发现时的实际路径 ID 精确查找，防止大小写不敏感文件系统接受未注册拼写；插件目录注册平面文件名，限定 ID 不接受嵌套路径。目录与解析保留 native/plugin 优先、CLI 最后补缺的来源顺序，所有来源统一应用 toggle，父 filter 在正式 ID 上应用。

复用已持久化 Spawn.capability_mode 作为原生命周期出生权限事实。恢复必须具备合法该事实，缺失或损坏时在整树 preflight 阻止恢复，不能把 admission 失败变成原 child 的 terminal；不修改原 execution_recipe，以保留 adoption 身份一致性。原出生权限再受当前直接父 ceiling 限制。当前 profile 仍决定工具资格；逐次 permit 不持久化，MCP 仍重建当前有效绑定，不能复用旧连接 incarnation。

普通 AgentRebuildSpec 保留 CLI 定义输入并传给 AgentBuilder，预加载 Workflow 名称不受影响。不新增独立权限框架。

## Verification

正式/别名 toggle 与 allow/deny、未知及同名优先级；插件和 CLI-only 目录关闭/重开及实际重建；真实 root/child 挂起持久化、旧 actor 退出、新 owner 恢复原 child 身份、恢复后工具逐调用授权与后代 ceiling。检查缺失/无效出生事实失败路径。

## Review boundary

上一轮 R2 的 resolver 探针证明出生权限重算，但并未证明旧版本能越过全部恢复屏障实际执行扩大权限的调用。`runtime_source_hash` 还包含有效 Agent 定义及能力源，会对部分漂移 fail closed；本次修复必须同时保证原 child 能正确恢复且不重授出生权限，不能通过放松 source hash 让测试通过。MCP 的旧 client incarnation 不作为可恢复 permit；跨进程出生 MCP 成员集合的持久化语义独立登记，不扩大此次 RWX 修复范围。
