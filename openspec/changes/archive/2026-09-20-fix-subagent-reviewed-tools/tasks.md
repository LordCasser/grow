## 1. Capability 与审核证据

- [x] 1.1 在 tool descriptor/registry/bridge 增加默认关闭的 child review policy，`write` 声明 Required；用 tool-protocol、tools 和 capability 单元测试验证默认值、最终投影与目录分类。
- [x] 1.2 让 review-required native identity 在 child 中 hard-eligible 但永不进入 initial-RWX fast path；用 ReadWrite/All write、普通 search_replace、final-filter rejection 场景验证。
- [x] 1.3 从 Shell frozen call 构造 exact identity/hash/bounded write summary，并让 PermissionManager classifier/audit 使用它；用长内容、重命名 identity、缺省 evidence 回退测试验证。

## 2. 执行闭环与文档

- [x] 2.1 增加真实 actor 回归，验证 Auto allow/deny、Ask/AlwaysApprove 既有语义、deny 优先、一次性 permit、参数或 epoch 改变时不执行。
- [x] 2.2 更新 Agent/Behavior 架构说明，明确 authored identity、descriptor review、initial RWX、permission mode 与 permit 的边界；运行定向格式、测试和 lint/check。
- [x] 2.3 记录验证证据，勾选实际完成 tasks，运行 `openspec validate --all --strict --no-interactive`，归档 change 后再次校验主规范和 archive。
