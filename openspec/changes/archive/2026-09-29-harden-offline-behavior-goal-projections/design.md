# 设计

Behavior 是会话选择，turn 保留 admission 时捕获的协议；Goal status 和 foreground terminal 是不同事实。离线展示必须按记录分别消费，不能从 cancelled 推导 Goal paused，也不能从 Normal 推导 Workflow cancelled。

`TranscriptProjection` 接住 `CurrentModeUpdate`，维护唯一当前模式/Plan phase，输出有限的只读模式记录并供播放器状态栏读取；不经 App 输入、effect 或请求通道。`ControlStateUpdate` 只展示已记录 applied/rejected terminal，pending/applying/superseded 不变成可交互控制。模式/Goal/Workflow 更新不切开正在播放的同一 prompt 正文。

Goal 从按 ID 的 notice map 收敛为单个当前投影与已撤销身份集合。clear 的空 ID 清除当前 Goal；替换/clear 后旧 ID 不复活，暂停后同 ID restart 仍允许。Goal 只是状态展示，生命周期 UiNotice 继续负责历史确认。时间和用量取记录值，不启动本地 Goal 时钟，不推导预算或 continuation。

Workflow 延用 live ingest 的 revision 规则，并对旧正 revision 的 clear 同样设护栏；保留一个 Run 的原条目，不因 Behavior 离开而结束 Run。无 revision clear 仍清理运行标记并留下 tombstone，不借此修改 Workflow 执行。

来源层复用 `SessionControlSnapshot::latest_from_timeline` 校验每个 Control，再使用现有 `build_goal_updated/build_goal_cleared` 纯函数恢复截点处的最终状态。补投影置于记录末尾、明确标为 snapshot/estimated；其作用是使最终展示与 authority 对齐，不能重建缺失中间事件的时序。无 Control 的 source 不发明模式/Goal 状态。

验证分别检查纯投影中间状态、Player 暂停/调速/最终 parity、真实 Timeline 校验与只读性，以及原执行状态机现有定向回归。审计中的范围外缺口单独列出。
