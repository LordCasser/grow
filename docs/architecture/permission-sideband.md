# 权限请求与独立裁决

权限行为契约以 [tool-authorization](../../openspec/specs/tool-authorization/spec.md) 为准。本页说明 Shell、workspace 和 provider 的实际分工。

## 请求路径

`SessionActor` 在冻结工具参数后构造 `PermissionRequestContext`，用 `PermissionRequestSource::Primary` 或 `Child` 明确权限域，并调用 `PermissionHandle::request_with_context`。`subagent_type` 只是展示字段，不决定来源。共享 `PermissionManager` 保留主域、每个 child 的独立作用域和撤销 token；短命令循环只分发请求，模型裁决和人工提示在各自的异步任务内等待。

Child Auto 的模型裁决经 `LlmPermissionClassifier` 的一次性响应通道送往主会话的 classifier worker。worker 为每个已接纳的裁决创建独立 `SidebandRun`，使用同一主会话模型路由，但不占用前台 turn。前台采样、其他 child 裁决和权限控制命令可继续运行；provider 耗时仍计入该权限请求的总截止时间。Sideband 的终结证据和用量必须完成持久化，超时后的修复任务也继续持有并发名额。

## 准入与失败

共享 manager 在入队前检查在途总量：child 请求阈值为 64，主 Agent 请求总上限为 72，给 child 饱和时的主 Agent 留 8 个名额，同时限制全部请求。名额随请求命令保留到 actor 丢弃或完成它，避免调用方反复取消后在无界通道留下无界积压。classifier worker 最多同时持有 16 个权限 Sideband；满额时立即返回 typed `Overloaded`，不等待已有 provider。已准入的 Sideband 仍并行。超载计数只有 `request`、`sideband` 两种固定标签。

Child Auto 的不可用、超时和超载都不授权工具，也不转为人工提示；主 Agent Auto 走现有不可用后的人工确认路径。模型给出的自由文本 `reason` 属于不可信裁决证据；Block 的 Agent 结果与权限审计使用 harness 固定理由，审计另保留结构化来源、verdict 和触发原因。

## 重置与 Goal

Reset 先在 manager 内存中撤销主域和 child 作用域，再在独立任务中通过既有写锁保存最新根状态。Reset 的成功回执等到写入返回；失败向调用方返回 I/O 错误，Shell 发送仅供 UI 的提示。失败后当前进程的撤销仍生效，但磁盘上的旧授权可能在重启后出现。

权限 Sideband 与前台尝试若已在 Goal 预算阈值前准入，各自按已确认的实际用量完整且仅一次结算。累计用量达到阈值后停止新准入；这不是对尚未返回的并行请求做预留式硬上限。详见 [behavior-goal](../../openspec/specs/behavior-goal/spec.md)。
