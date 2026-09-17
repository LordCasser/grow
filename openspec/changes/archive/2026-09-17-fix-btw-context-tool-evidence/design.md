## Context

`grow/btw` 根据请求 session id 发送 `SessionCommand::SideQuestion`，run loop 在同一 SessionActor 上启动独立任务。`materialize_timeline` 在一个 ChatState 命令内返回当前已提交 Surface 和 high-water input ref；问题发生在后续的无条件尾部 pop 循环，而不是快照来源。

`sampling-types::project_portable_history` 已有中性投影规则：保留正文、合法且唯一的调用和相邻结果/附件，过滤未配对或有歧义的协议，清除 reasoning 和原生 continuation 元数据。默认无 native_continuation 的 wire builder 不会自动调用该投影，因此 `/btw` 需要显式使用它。

## Goals / Non-Goals

目标是让独立旁路读到冻结边界之前已经提交的执行证据，并继续兼容三种 provider 协议。保持只读、无工具、固定 route/budget/source refs 和既有 overload 重试。

不补采集尚未进入主 Surface 的子任务内部进度，不等待主 turn 完成，不将缺失结果伪造成成功或取消，不更改共享 projector 行为及其他 Sideband。

## Decisions

直接以 `project_portable_history(&materialized.surface)` 替换 strip reasoning + 尾部 pop，沿用现有身份、参数与相邻配对规则；不新建另一份 `/btw` 协议修复器。与只判断最后一组是否完整相比，共享投影还能保留部分完成组中的真实证据与带调用的正文。

先写真实 actor/provider 回归并确认旧代码失败，再做最小生产修复。测试使用本地 MockInferenceServer 捕获 Chat Completions、Responses、Messages 的请求，覆盖完整连续尾部、部分完成、完全未完成以及原始 Surface 不变；overload 重试期间推进主上下文以核对冻结边界。现有 sampling-types 中复制裁尾算法的测试改为验证共享投影，避免错误算法自证正确。

## Risks / Trade-offs

- 保留此前误删的结果增加实际请求大小 → 沿用当前模型窗口与 Sideband 预算，不通过丢掉最新证据控制大小。
- 冻结之后完成的执行不在本次回答中 → 保持快照语义；Sideband 没有实时核对能力。
- 无效或歧义工具协议仍被既有 projector 过滤 → 本次不放宽安全配对规则，保留原 Timeline。
