## Boundaries

响应终止是 provider 事实，Turn::Ended 是宿主生命周期事实，任务/Goal 是否完成由其自身契约负责。provider 正常结束没有新的可执行调用时，普通 Turn 可以结束；不因句子像预告或缺少额外工具而自动重试。provider 响应若含完整业务调用，仍按权限和调用完整性执行后续步骤，不把 response.completed 等同整个工具循环结束。

## Provider facts

以带 backend 标签的 ProviderTerminal 保存 Chat finish_reason、Messages stop_reason/stop_sequence，以及 Responses terminal event/status/incomplete reason。替代分散的原始终止字段，保留中性 StopReason 供控制流程使用；两者不从文本互相猜测。移除“有调用就将中性终止覆写 ToolCalls”的规则。原始 HTTP artifact 保持不可变。收到原生终止之后即使候选校验失败或被宿主拒收，attempt evidence 也保留所观察的 terminal；缺少终止时保持缺失，HTTP EOF/[DONE] 不冒充 provider 正常完成。

## Host facts

在既有 Request::Completed 保存最终候选的 provider terminal 与 attempt，绑定已有 request id；请求的路由证据来自该 request 的快照，不能用当前模型重建过去原因。TurnTerminal 增加带标签的来源：Provider（关联 request）、Host、UserCancellation、Recovery。既有 stop_reason/completion_kind 是宿主分类标签，不作为 provider 字段解释；不再新增可与 source 矛盾的 synthetic 布尔值。

历史缺少来源/terminal 的记录保持 Unknown/None，不能凭旧 end_turn 补造来源。不建立兼容转换框架，不重写用户 Timeline。每个新生产路径必须显式给出来源。provider 已正常结束不妨碍宿主因后续持久化失败、取消或崩溃关闭自身 Turn；两层记录并存，宿主事件不能覆盖原生事实。

## Termination and recovery

拒绝优先于工具派发：记录完整调用及宿主拒绝执行的配对结果，不能重放或遗留悬空调用。截断/pause/context overflow 的原始原因保留；完整业务调用可按既有协议处理，其后的继续是宿主工具循环而不是改写 provider 原因。取消/owner/预算保持最高准入边界。错误补全记录真实宿主原因，不能成为成功候选。

FinishTurn 整体撤下，其虚构的 waiting 标签和计数也随之删除；等待复用现有工具/通知/Goal 协议，不再另建等待调度系统。自定义 Agent 已显式配置的 completion_requirement 和 Stop hook 仍是独立的宿主继续理由。

## Verification

三 API 的自然结束（包含冒号正文）只请求一次；工具响应继续且原因不改写；拒绝与完整工具同响应不执行；缺终态不成功；每种终态的 request/attempt/source 可追溯。检查模型/endpoint 切换、插话、取消、Goal 预算、压缩和恢复事件不伪造来源。复用 target，限制并行构建并检查磁盘。

## 补足方案盲区

- host terminal 必须独立收尾，即使 provider 已发过 terminal；不能把“不得改写 provider 事实”误实现为“不得再发 Turn::Ended”。
- 观察到终止、接纳候选和完成用户任务是三个不同判断。工具参数校验失败仍保留已观察事实，拒绝不能被完整调用升级为正常完成。
- source 采用带标签枚举；不再增加 redundant synthetic 布尔值。request 引用只允许关联本 Turn 中含原生终止且有 attempt 编号的 Completed，重放同样校验。
- 同步模型/端点切换不能改写先前 request 的 backend；真实切换期间的响应通过原 request id 归属，下一次生成用新路由。
- “cancelled” 也不是用户行为的充分证据：达到回合上限、Hook 拒绝、权限超时、失去准入和宿主 shutdown/control 属于 Host。
- 不再制造 waiting_for_user/background 这种没有唤醒依赖的完成标签。保留既有真实等待工具及通知协议；Goal 继续有明确的自主调度契约。
- 测试夹具不能依赖被撤下的 FinishTurn 识别前台，否则会静默跳过工具场景；协调 mock 改用该测试原本需要的 list_active_sessions 能力。

旧 v2.1.6 changelog 与已归档 FinishTurn change 仅作为发布/审计历史保留；本次并未修改已安装的 Grow 二进制。

## 验证中发现的夹具偏差

压缩测试仍索引已删除 FinishTurn 的第三条结果，现保留并验证两条业务结果。
`turn_pipeline_v2_tests` 的显式线程栈仍为 8 MiB，但 `spawn.rs` 在此前 release prepare 已将 debug 会话栈设为 32 MiB（release 为 8 MiB）。测试按 debug 实际运行条件同步为 32 MiB，未改变生产栈或跳过首次 Goal bootstrap 场景。
