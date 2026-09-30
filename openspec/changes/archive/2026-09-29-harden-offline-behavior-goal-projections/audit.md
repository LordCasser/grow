# Behavior / Goal 离线状态专项核对

核对时间：2026-09-29。此记录接续上轮中断审计。本轮针对已知展示缺口作最小修复，不修改真实执行状态机。

## 事实链

`SessionControlSnapshot` 是 Behavior、Plan runtime 与 Goal 的持久权威。Timeline `Control` 同时提交选择、Goal 状态及对应模型上下文；用户输入获得 foreground 时捕获的协议不会被之后的选择重标。Actor 的 `enqueue_current_mode_update_inner` 把 CurrentModeUpdate 经 FIFO 写入 updates；GoalNotifySender 同样以 persist=true 发布 GoalUpdated。Control pending/applying、picker availability 和 pending interaction 是 transient，不能承诺从磁盘逐帧还原。

新独立 export/replay 使用 `storage/transcript.rs::project_session`，它保留 Grow 事件；旧的 ACP-only replay reader 丢弃 Grow 的实现不能作为新入口的证据。实时 Pager 的 `AgentView` 负责应用效果，离线 `TranscriptProjection` 只负责信息展示。

## 核对矩阵

| 情形 | 执行侧事实 | 原离线问题 | 本次展示规则 |
| --- | --- | --- | --- |
| Normal / Clarify 切换 | CurrentModeUpdate 表示当前选择 | tracker 忽略 mode | 有限模式记录 + replay 状态栏，不运行行为逻辑 |
| 普通 turn 中设置 Goal | Control 原子提交 Goal Behavior 与目标；旧 turn 保留 admission identity | 模式不可见 | 模式/Goal 可见，旧正文继续属于原 prompt |
| Goal turn 完成后自动续轮 | Active 才由 idle arbiter 产生后续 turn；内部输入有隐藏元数据 | 没有组合测试 | 仅播放已记录的续轮和回复；不生成新的输入，不泄漏隐藏 directive |
| 只取消 turn，Goal 仍 Active | Turn terminal 不等于 Goal paused | 容易在分析中混同两种 stop | 记录 cancelled，但 Goal 保持已记录 Active；播放器仍可 Finished |
| 暂停 Goal | Goal Paused 与 Normal Behavior 同 Control 提交 | Goal 状态可见但 Normal 切换不可见 | 分别呈现；用量晚到只更新记录值 |
| Blocked / BudgetLimited / Complete | 停止 Goal，释放 Goal Behavior；目标记录仍存在 | 缺少各 stopped 状态交错验证 | 保留目标、停态、预算与累计用量，不据正文或 tool title 推断状态 |
| Paused 后 restart | 同 goal_id 新 definition/control 状态可再 Active | 不能把一般 stopped 都当 clear tombstone | 同 ID restart 允许；仅 clear/替换注销旧 ID |
| Clear Goal | Control.goal=None；GoalUpdated.cleared 的 goal_id 固定为空 | map.remove(空ID) 既删不了映射，也没删旧 Notice | 清当前 Goal 展示并保留已注销 identity；生命周期命令 notice 不受影响 |
| 新 Goal 替换旧 Goal | 当前长目标唯一 | 离线 map 可保留多个“当前目标” | 单当前目标；旧 ID 的迟到 projection 不复活 |
| Goal definition 编辑 | Control 含 definition_revision；GoalUpdated 只含稳定 goal_id 与新 objective | UI DTO 不提供每次修订 identity | 以已记录最新 objective/状态更新当前展示，不虚构 revision 历史 |
| Plan 四阶段 | drafting / awaiting_approval / executing / amending 在 mode metadata / Control 内 | phase 不可见 | 按记录展示 phase；不自动批准，不打开可提交交互 |
| 离开未完成 Plan 要确认 / 拒绝 | CurrentModeUpdate.current 保持 Plan，requested target 不等于 applied | 确认/拒绝反馈被忽略 | 保持 current mode，只展示已有说明及 Applied/Rejected receipt |
| 离开 Workflow Behavior | Behavior 不拥有公共 Run 的生命周期 | 容易错误推导 Run 结束 | 只改变模式，Run 保持自身记录状态 |
| Workflow 晚到/重复 revision | live ingest 有 revision guard | 离线没有 guard，完成可退为 active | 旧正 revision（含 clear）丢弃；保留无 revision clear 的既有投影语义与 tombstone，不新建重复行 |
| 切换发生在模拟流式中 | 当前模式与当前正文属于不同维度 | mode 被当依赖边界，可能提前补完动画 | mode/control/Goal/Workflow 更新均可穿插，同一 prompt 正文不被拆分 |
| 最后一次控制的展示 cache 丢失 | Timeline Control 已提交，updates 可滞后 | 读取成功却停在旧 Active/旧模式 | 末尾恢复 canonical endpoint，明确标为 captured snapshot / estimated；不猜中间位置 |
| Control 损坏 | architecture、revision、retired layers、Goal ownership 都须校验 | Timeline 泛型折叠不等于 Shell snapshot 校验 | 复用 latest_from_timeline 的完整校验，失败即拒绝读源 |

## 实现边界

- 离线只添加展示状态，不构造 BehaviorCoordinator、SessionActor、GoalDrive、工具 runtime 或控制 effect。snapshot 使用已有纯 Goal DTO 转换。
- 状态 hydration 与历史确认有不同作用：GoalUpdated 更新一个当前状态块，clear 删除该状态块；持久 UiNotice / Control terminal 保留操作历史。不会把每次 token checkpoint 变成一条“目标重新启动”记录。
- 回放时间与 Goal elapsed 都取记录值。没有预算时显示无预算；未知用量是下界，不补零为精确用量。Goal 的局部 UI 秒表不在离线启动。
- Workflow 的正 revision clear 也受旧版本护栏保护。实时 `workflow_ingest.rs` 对 clear 跳过该护栏，是另一个消费者的潜在回退边界；单独列入 backlog，不顺带改变 live 语义。

## 仍不能承诺的部分

1. 末尾 snapshot 解决“截点状态错误”，不解决中间 cache 丢失后的精确时序。未保存的 confirmation modal、等待审批时的光标/按键、picker availability、队列草稿不会被发明出来。
2. Workflow 的完整 Timeline/journal 历史恢复不在本轮；Goal/Behavior endpoint 的缺口已关闭，普通输入、turn terminal 与 Workflow 中间 cache 缺失仍需独立方案。
3. GoalUpdated 不携带 control/definition revision；在失去 Timeline 的畸形混合日志中，不能仅靠时间戳证明所有同 ID 状态的顺序。正常来源以 committed updates 顺序及捕获 Control endpoint 为准。
4. 各 source 有自己的捕获前沿，仍不是跨 agent 全局原子快照；也不是执行器取消/预算并发正确性的全量证明。本轮回归执行侧现有测试，实际改动仅在离线消费者。

运行结果和场景对应见 [verification.md](verification.md)。
