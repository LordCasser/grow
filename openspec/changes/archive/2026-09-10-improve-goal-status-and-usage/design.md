## Context

实际会话 `01a08906-7afd-70e2-af4e-5ff9ea4db84c` 的 26 次 `get_goal` 均在 70–340 ms 内成功，23 次发生在 step 0。`updates.jsonl` 有初始工具行、标题和权限投影，却没有完成事件。`acp_conversion::acp_tool_update` 只处理 UpdateGoal，GetGoal/CreateGoal 落入 `_ => None`；`tool/result.rs` 因而不发送完成 ToolCallUpdate。Pager 的 pending_tools 留住运行行，直到 `finish_turn` 才停止计时。这是界面长计时的已证实机制，不能解释成读取 Goal 数据用了 21 秒。

`render_goal_continuation_from` 已注入目标、预算、已用 token。模型重复沿“审计 → 读取 Goal → 工作”的路径调用工具；没有 UI 轮询证据。实际 Goal 命令仍由 Session actor 处理，子会话读继承快照。本次保留这个所有权，不因为最初的排队猜测拆分查询路径。

Usage 来自 chat-state 的进程内 `session_usage`，包含主采样和已折叠子任务，在新进程恢复时重新计数。Goal 来自独立持久化 tracker，跨恢复累计。Context 是窗口压力，不是 token 消耗。`session_usage_block_text` 已使用 `group_thousands`，`goal_detail::usage_lines` 直接插值是格式缺口。Pager 状态栏只给 `goal_state` 建立了消耗入口，没有普通账本投影。

## Decisions

1. 在既有 ACP 转换中补上 GetGoal/CreateGoal，与 UpdateGoal 共用 Completed 和 raw_output 的投影。模型结果、Timeline 事件和 UI 工具状态由同一成功返回收尾，不通过 UI 超时猜测完成。用真实转换函数输出驱动 Pager tracker 回归，验证 turn 未结束时就清除运行状态。
2. 不加 TTL 缓存、第二份 Goal 状态、硬编码超时或后台轮询。明确提示只在缺少上下文、需要最新预算或显式核对时调用 `get_goal`；业务完成仍由原有完成审计和 `update_goal` 验证。
3. 在 chat-state 现有账本的主调用、子任务折叠、不完整标记更新后发出累计账本快照事件。shell 用现有 transient 通知向客户端投影；Pager 只替换最新累计值，不逐次相加。重新连接要读取当前账本，不能把历史 turn usage 当作当前进程的累计值。复用 `PromptUsage` 携带与 `/usage` 一致的总计和模型分类，无第二套计费模型。连接时随现有状态 advertisement 补发当前账本；客户端拒绝同窗口内倒退的快照，reload 清空窗口。
4. 普通主会话 Agent 状态栏在 Goal 所在插槽显示紧凑 token 和 cache hit 百分比，使用总 cached input / 总 input；无输入或非法比例为 N/A，不完整总额保留 ≥，命中率说明 recorded。点击走 `Action::ShowUsage`。Goal 存在时保留其详情入口。紧凑栏保留 k/M，完整数字只在详情中使用逗号。窄终端的绘制与点击区域必须限定在可见宽度内。既有 Usage dispatch 固定以主会话为目标，子 Agent 内嵌视图暂不增加错误地指向父账本的入口；子视图的三页诊断面板路由作为独立债务登记。

## Failure and recovery

用量事件只服务 UI，不增加模型上下文、Timeline 事实或计费写入。当前进程账本读取失败不能显示为精确零。新进程恢复、同进程重新连接、子任务晚到结算分别遵循现有账本窗口和 owner；不得累加 prompt 的 Usage 来伪造 session 总额。Goal 查询与权限行为不变，不以提示修改保证模型绝不重复查询。

## Validation

- 覆盖 Create/Get/Update 的 ACP 完成状态、原 call id 和结果；以初始 get_goal、标题更新、真实转换结果的顺序验证 Pager 在当前 turn 内停止计时。
- 检查续跑提示与工具说明的按需读取语义，不用测试宣称模型必定遵从。
- 检查账本主调用、子任务及 incomplete 更新的累计投影，恢复不读历史 token；通知不触发新请求或上下文修改。
- Pager 测试覆盖无调用、完整/不完整、异常缓存比例、Goal 优先、点击 Usage、窄宽度和详细大数格式。
- 运行相关 Rust 检查及 OpenSpec 全量严格校验，记录真实通过、失败和未执行部分，再归档。
