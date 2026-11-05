# Grow 全仓与版本演进架构审视

本次审视覆盖当前57个workspace package、非workspace vendor/fuzz、脚本、CI、产品入口和文档契约，以及v1.0.0至v2.6.0全部46个发布标签。远端origin标签与本地一致；另核对两条未合入旁支和上游导入边界。各分区按实际Git对象、入口、调用方、owner和相关测试审查，不以文件大小、trait数量或关键词命中推导架构缺陷。

七份报告记录30项当前问题条目、2项容量/性能风险，以及独立文档和用途待确认债务；这些条目的证据强度不同。父侧从实际生产库公共API确认了索引删除丢失、gitfile目标误删、hunk外部编辑丢失与失败投影不一致；测试执行器有真实隔离实验，其他事项保留静态链或机制实验边界。完成审视不表示已修复这些问题，也不表示每个旧版本/平台都通过运行测试。

## 当前架构判断

主要状态所有者已有清楚分工：CLI/Pager/ACP是产品和传输入口；Shell SessionActor拥有接纳、前台Step、控制和各领域编排；ChatStateActor/Timeline是持久事实权威；Sampler管理provider attempt与结算；ToolBridge、PermissionManager、Hooks、MCP和Workspace分别拥有工具资源、授权、副作用、连接目录与文件系统能力。Pager是投影，Config/Agent提供定义和候选快照。当前没有证据支持再建立统一调度、配置或授权框架。

剩余问题主要发生在这些owner之间：恢复层自行重建输入路由和队列身份；会话已建立独立快照却有消费者读取进程共享快照；异步准备完成后没有复核对象代际；把可读、已发channel或日志警告当作已持久完成；测试绕过真实入口而只证明helper。它们需要修正具体边界和提交顺序，不能靠新增抽象层、默认重试或放松底层校验解决。

| 跨区主题 | 代表发现 | 收敛方向 |
| --- | --- | --- |
| 派生状态与权威事实分歧 | Session F1/F2/F3，WS-01，PB-01/02 | 保留canonical route、队列位置和请求身份；恢复/投影消费同一事实，不维护另一套不完整fold |
| 会话/任务/连接代际混用 | AC-03，SAMPLER-1，MCP-F1/F2 | 使用原owner提供的会话快照/任务身份/注册连接身份；await后复核当前资格，不按名称切换目标 |
| 失败先提交或错误成功 | AC-01/07/08，WS-06b，Session F4，PB-07 | 错误传播与durable边界一致；失败保留可恢复证据；成功只表示已完成该操作实际承诺 |
| 资源寿命没有完整owner | PB-03/04/05，ACP-1 | 明确owned child/文件/queue的结束和容量边界；无法确认回收时如实报告，不扩大到全局清理权限 |
| 内容身份过弱 | WS-02/04/05，PB-06 | 内容摘要、仓库身份、路径参数和历史IP各守自身语义，不以字符串或时间近似替代实体/内容身份 |

## 覆盖与分区交付

[Inventory](inventory.md)列出全部模块、发布commit、历史独立模块去向和旁支身份。[验证记录](verification.md)给出父侧实际公共API输入/输出与限制。每份分区均包含全部46tag的tree/manifest/相邻diff或明确复用依据，版本表不是逐行或逐版本运行通过声明。

| 分区 | 当前结论与重点 | 详细证据 |
| --- | --- | --- |
| Session/Timeline/Workflow | 5项：挂起steer冷恢复、编辑FIFO、稳定重试、journal恢复屏障、冲突completion | [session-runtime](reviews/session-runtime.md) |
| Sampling/model/compaction/auth/http | 1项：按RequestId清理缺少task代际；普通shell随机ID降低当前触发面 | [sampling](reviews/sampling.md) |
| Tools/authorization/sandbox/hooks/MCP | 3项：初始化后资格窗口、SDK reverse路由身份、SDK重名整组校验；另有主规范旧retry条款冲突 | [tools-authorization](reviews/tools-authorization-recovery.md) |
| Agent/config/plugin/extensions | 8项：配置旁路写、空插件Hook残留、session/shared快照、规则alias、Agent链接环、Skills配置交付、插件更新/错误语义 | [agent-config](reviews/agent-config.md) |
| Workspace/storage/index/memory | 6项：删除归约、摘要memo、worktree gitfile、memory身份、envrc参数、hunk并发/失败 | [workspace-storage](reviews/workspace-storage.md) |
| Client/UI/terminal/render | 未确认当前会话串线或终端泄漏；2项独立风险为ACP容量和开放fence复制 | [client-ui](reviews/client-ui.md) |
| Product/build/platform/quality/vendor | 7项：bridge缓存2项、PTY生命周期、exact清理判据、启用crash capture的3项；原生vendor与发布门禁有实际验证 | [product-build](reviews/product-build.md) |

## 建议先处理的独立修复

先处理可能损失用户数据或继续执行被撤销组件的边界：WS-03工作树删除身份、WS-06a hunk外部修改、AC-01配置事务、AC-02插件Hook撤销、AC-03a LSP快照、MCP-F1初始化后资格复核，以及Session F1合法挂起冷恢复。每项按已有owner分别建change/修复任务，使用报告的触发和对照作为通过标准。WS-06b失败投影、PB-01/02恢复身份、Session F2/F3等随后独立处理。证据不足的负载/性能假说先测量，不给未经验证的全局上限或故障断言。

完整登记见 [backlog](../../../backlog.md)。AC-03a/03b、WS-06a/06b已按可独立验收的行为拆开；插件安装与卸载的持久错误也要求分别派发。旁支MCP修正只能作为机制和夹具参考：main同时有更新的typed/no-replay/分页边界，不能整支覆盖。当前no-replay是正确实现，不为符合旧规范文字而重新发送可能已产生副作用的tools/call。

## 版本演进中的已修复边界

历史审查识别了真实收敛：v1.1.0取消无必要的proto/远程工具服务和纯镜像，合并拆分过细的session/lifecycle层；v2.0开始Timeline统一事实和权限/Behavior状态；v2.1.5修复Settings、Hook进程/DNS、permission与向量索引一致性；v2.2.1权限actor改为request-local且可撤销；v2.4.1起发布门禁绑定同一完整commit；v2.6保留完整长插话、加强进程/资源和观察边界。各报告给出具体代码/提交和首个tag，不把早期已删除或修复路径列成当前缺陷。

本轮model/effort特性的现状review另发现普通resume错误采用出生Spawn配置，已在该功能change中修正为来源child最后durable selection；新工具只等actor接纳以免Step自锁，命令类型归session契约。该特性最终运行/独立review证据在 [allow-session-model-effort-control](../2026-11-05-allow-session-model-effort-control/verification.md)，不代替此处全仓结论。活动区最终复核仍以该change实际交付版本和测试为准。

## 证据与交付限制

审查对关键入口/数据链逐函数核对，对全部模块族建立职责/依赖/生命周期表，对全部标签建立结构和变化证据；没有穷举所有历史提交每行、所有语言query、平台kernel行为、真实网络服务、外部发布或每版重编。没有进行Git提交、合并旁支、发布或清理用户工作。

一次工具权限reviewer续跑被运行环境的Timeline/compaction持久化校验阻断，结果无法完成确认；父侧没有把它算成通过或已清理，而是用独立补审及不同报告路径完成同一覆盖。该runtime最后观察仍为finalization_pending，具体诊断保留在本会话工具状态，尚不能从此归因当前仓库某一源码版本。审计不靠该失败结果提供任何发现证据。

全部分区已接受；57模块映射和七份46标签表独立核对通过，活动特性已完成实际CLI组合验证，格式与链接检查通过。两change均已归档，最终全量strict 16/16、全部archived 848/848通过；本报告的完成标准是审视、证据与后续边界明确，独立债务并未关闭。
