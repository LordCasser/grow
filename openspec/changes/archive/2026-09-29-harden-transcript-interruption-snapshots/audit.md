# Export / replay 对抗性审计

审计日期：2026-09-29。范围是前序 transcript tree export 与只读 TUI replay，不包含取消执行器、provider、实时恢复协议的重设计。

## 判断依据

正确性不能只比较最终正文。一次自然打断至少涉及四条各自独立的边界：用户 turn、模型响应 admission、工具 terminal、子 agent / 后台任务 lifecycle。前一条结束不代表后一条成功。播放动画没有事实 authority，不能改变这些边界的交付顺序。

存储同样有两层：Timeline 是事实，updates 是展示序列与可重建响应 cache。普通追加、尾部半行、缺失 cache、已提交坏行必须分别处理。读取开始时的文件长度，只有实际约束后续读取才称得上截点；事后比较两次长度无法提供这个保证。

核对入口包括：`cli/src/main.rs` 的离线 dispatch、`shell/session/storage/transcript.rs` 与 `response_projection.rs`、`SessionActor` 的 `updates` / `notification_drain` / `hook_dispatch`、`agent/subagent` 的事实投影，以及 Pager 的 tracker、live session notification、transcript projection、播放器和输出发布路径。旧 change 的正常/空会话验证只能证明其对应场景。

## 发现与处理

| 场景 | 原实现盲区 / 失败方式 | 本次处理 | 验证证据 |
| --- | --- | --- | --- |
| 正文/工具途中取消，再发下一问题 | 通用 finish_turn 与 user message 处理清空 pending 工具；终态原因没展示 | 离线 turn fence 保留工具所有权，显示 recorded stop reason；工具只由自己的 terminal 收束 | projection 的 cancelled / cross_turn 测试；真实 CLI fixture |
| 旧 prompt terminal 晚到 | 无条件 finish_turn 拆开新 prompt 的回复 | 按 prompt identity 收束，重复 terminal 只显示一次 | late_old_terminal 测试检查 EntryId 和正文 AB |
| 用户 echo 重复到达 | 即使行去重，用户边界仍可能切断流 | 已显示的 message identity 在边界处理前去重 | duplicate_user_echo 测试 |
| 隐藏的内部用户输入 | 不展示正文仍需分隔 turn | 保留原隐藏语义，使用元数据区分回复归属 | hidden_user_echo 测试 |
| 前台命令转后台并跨 turn 完成 | Started 被改为 Completed 后再追加另一 Completed；前台 pending 残留 | 沿用前台 EntryId 降级为后台开始事实，抑制后续前台 ACP 更新；独立唯一 terminal | background_task 测试 |
| 模拟文字尚未完成时取消/继续/开始工具 | 播放器等动画完成才交付依赖事件 | 到期边界立即补完此前允许显示的正文，再交付事实 | cancellation/new_user/tool_boundary 中间状态断言 |
| 暂停期间有到期取消；调速后继续 | 最终正文相等不能证明中间状态冻结 | 单一虚拟时钟同时管理事件与 reveal | pause_freezes / speed_change 测试 |
| 粘贴含 q、空格、+-、slash command | 普通终端按键流可误触播放快捷键 | bracketed paste，忽略整体 Paste 事件 | 事件测试 + 真实 PTY |
| canonical response 替换 preview anchor | 重新序列化时间/缺失 prompt metadata 改变归属和播放时间 | 保留同 attempt 的原始时钟及 prompt 元数据；仍由 admission 决定正文 | canonical_response_retains_anchor_time 测试 |
| 取消后 rewind 再继续 | 按原始日志简单串接会带入废弃分支 | 复用既有 rewind 与 response reconciliation，不另造分支算法 | rewind_after_cancel 测试 + response_projection 回归 |
| 导出时 writer 追加，或补齐原半行 | 事后 len 检查既误拒绝正常追加，也不能阻止读取越界 | 固定已打开文件身份/字节上界，之后的换行也不可见 | captured_updates_ignore_completed_torn_tail 测试 |
| 树发现后又 spawn child | 第二次读取 Timeline/body 可能混入不同截点 | 树拓扑和正文共用捕获 | topology 测试检查 nodes 和 restored spawn |
| 已捕获 ledger 被截断 | 可能静默输出不完整前缀 | bounded reader 检查原长度；显式错误 | updates / Timeline 截断用例 |
| 外部 session 的行恰好位于 rewind 废弃区 | 先过滤可能掩盖身份污染 | 所有原始 committed envelope 先核对 session identity | foreign_session_update 测试 |
| Timeline 有事实而 updates 为空/不存在 | 返回空 transcript 造成假成功 | 明确报错；仅身份事实允许空会话 | empty_committed_display / missing_updates 测试 |
| 大来源、小 ledger 引用大 artifact | 只统计展示文件低估成本 | ledger 物化前累计字节，累计 Timeline/Sideband/updates/扩展投影记录；必要 artifact 同计 | sparse oversized source 测试；引用验证复用原检查 |
| completed Hook 展示从实时恢复临时生成 | 只消费 updates 丢失 Hook 历史 | 复用已有静态 Hook 转换补投影，标为 snapshot | completed_hook 测试（取消的 handler，不运行） |
| parent agent message 打断 | receipt 的通知本来是 transient；正文还需专门转换 | 从捕获 Timeline receipt 读取有界、校验过的 immutable body，复用 receipt 和 UiNotice 纯投影 | 源 receipt 测试 + interrupting_parent_message projection 测试 |
| receipt 的 body 已不可用 | 不能虚构正文或重投递消息 | 与 live reconnect 一样显示 Message unavailable，保留 identity / interrupt 事实 | 同一 source receipt 测试的缺失 artifact 分支 |
| 缺失 child start/end 展示 | 可见 lifecycle 与已验证树不一致 | 从 direct delegation / lifecycle owner 的已验证事实补回；终态存在时 start 插在前面 | 原树/身份测试与 root captured spawn 断言 |
| 目标已存在、写入中失败 | 半目录泄漏/覆盖旧文件 | 保持私有临时输出与 no-replace 发布 | 6 个 export 用例 + CLI 二次发布拒绝 |

所有补投影只使用事实/展示函数。没有构造 SessionActor，没有执行 Hook，没有创建 provider、调度工具或重新发送 agent message。工具的历史 command 只进入展示。

## 未闭合边界与后续工作

1. **部分展示 cache 缺口仍不能完整恢复。** response、Hook、direct child 与 parent receipt 已有纯离线投影；普通输入、turn terminal、Goal/Workflow 等尚无统一离线覆盖检查。完全缺失 updates 会失败，但部分缺失不能承诺检测齐全。需另一个 change 定义 Timeline 与 prompt/display identity 的精确映射，避免以时间邻近猜测。已登记 backlog。
2. **实时 unknown stop reason 分类。** live `AgentView::finalize_durable_turn_terminal` 只把 cancelled/error/rate_limit 视为非成功。此次离线展示保留原因，不顺带改 realtime Completed 的含义。已登记 backlog。
3. **跨 ledger / session 不提供全局原子事务。** 每个打开的 append-only ledger 有固定上界；跨来源前件若尚未出现会失败并要求重试。运行中 child 尚未持久化、完成事实与结果引用尚未对齐等不能靠猜测修补。不同节点不是同一墙钟瞬间。
4. **信任 append-only 与 immutable blob 存储契约。** 文件身份固定阻止换路径后读入新文件；长度检查能识别截断，但不是对恶意同长度覆写、截断后迅速原样长度恢复的并发取证机制。不宣称对任意外部文件破坏提供事务快照。
5. **动画是近似。** 原始 token 时序未持久化；同时间或很短间隔的多个依赖事件可能快速补完正文。补回 transient 展示无可靠跨 ledger 原顺序，标为估计并遵守已知依赖，不能还原精确历史交错。
6. **资源预算不是峰值内存指标。** 512 MiB 来源、250000 条累计记录、512 个 session / 32 层深度，以及逐条 JSONL 上限，约束输入规模；Timeline/JSON/Markdown 转换仍有内存放大。没有执行极限压力、磁盘故障注入或长期真实多 agent 负载测试。

## 验收方式

具体命令、结果与运行边界见 [verification.md](verification.md)。除最终 Markdown parity，还检查中间 running 状态、原 EntryId、事件去重、正文归属、播放边界，以及真实 CLI 的只读性和终端恢复。OpenSpec 格式校验不能替代这些测试。
