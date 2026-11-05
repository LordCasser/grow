## 1. Resume selection

- [x] 1.1 Task schema 与 runtime 传递 resume model/effort；工具测试覆盖四种组合、null、none 和非法输入。
- [x] 1.2 child runner 在 canonical source 校验后应用显式覆盖；生产 runner 测试验证 provider、Spawn/baseline、来源不变、失败及 checkpoint 边界。

## 2. Current session model tool

- [x] 2.1 注册 Grow `set_session_model` schema、IO 和绑定适配；验证拒绝空参数、未知字段及伪造目标。
- [x] 2.2 复用用户模型控制准备与 actor admission，在闭合 Step 应用；实际工具入口验证无自锁、下一请求、pending model + effort、错误和权限/Workflow 边界。

## 3. Feature review and delivery

- [x] 3.1 独立审查实际 diff 和共享控制/恢复调用方；逐项处理阻塞本次验收的发现并记录证据。
- [x] 3.2 运行相关 tools/shell 单测、真实 mock-provider 集成、默认 CLI check/build 与格式检查，记录最终版本命令和覆盖限制。
- [x] 3.3 同步子 Agent 用户指南、model/effort 工具说明和开发者契约链接，核对示例及旧说明。
- [x] 3.4 完成场景核对，执行全量 strict 校验、归档及 archive 校验。

## 4. Repository-wide architecture audit

- [x] 4.1 用户已扩展审视到整个仓库及各版本；另建最小审计 change，枚举当前模块与可见版本，形成逐项覆盖矩阵并链接专项发现。全仓审计在独立 change 交付，不能用此处特性 review 代替。
