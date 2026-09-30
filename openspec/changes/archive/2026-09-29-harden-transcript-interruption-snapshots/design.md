# 设计

## 证据与约束

- Shell 的 Timeline 为事实 authority，updates 为展示序列；response reconciliation 和 rewind 复用既有算法。
- transcript reader 目前 discovery/read_session 两次打开、两次读 Timeline，并仅比较文件长度；正常追加既可能触发失败，也可能在校验前无界读取。
- AcpUpdateTracker::finish_turn 与 handle_user_message 清空 pending_tools。实时客户端可依赖执行方终态，离线读取不能把缺失 terminal 推成工具成功。
- TranscriptProjection 无条件处理 TurnCompleted，忽略 prompt_id、stop_reason；Player::tick 等待动画完成才交付依赖事件。

## 方案

捕获层保存已打开的 session authority、固定 ledger 字节上界及经校验 Timeline。目录清单和正文从同一捕获读取；跨 session 不是全局原子快照，未闭合的跨 ledger 前件明确失败。提交后的追加不进入截点，尾部半行不可见，截断/已提交损坏失败；来源预算覆盖 ledger 与必要引用，记录预算覆盖展示与事实。

投影层不启动 App/executor。复用 tracker 处理正文/工具，增加仅离线使用的边界能力，以工具自己的 terminal 决定结束，保留未完成事实。TurnCompleted 记录原因且按 prompt identity 收束当前流；旧 terminal 只描述自己的 turn。后台 Started 行保持不可变事件语义，后续 terminal 不改写起点。

前台 Execute 转后台时沿用原 EntryId，移除前台 pending ownership，并抑制后续重复前台终态；后台的 Started 与 Completed 是两项事实。重复的用户 echo、turn terminal、child terminal 和后台 terminal 按已有稳定身份去重。

复用实时路径中已有的纯转换：UiNotice 的通信正文/receipt identity，以及 Timeline 的 completed Hook projection 和 direct child spawn/end。缺失展示的已验证 Hook、child lifecycle、parent/agent reply receipt 补投影；未知跨 ledger 顺序标为估计，不重新运行 handler、不启动 SessionActor、不读取 child 的执行输出文件。Receipt 正文只读现有有界并校验 hash 的 immutable artifact；缺失时保留与 live reconnect 相同的 Message unavailable 提示。Goal/Workflow 与普通输入/turn terminal 等完整离线重建超出本次最小闭环，偏差列入 audit 与 backlog。

canonical response 替换 preview anchor 时保留原 sampling attempt 的 prompt identity 和时间字段，避免使用重建 envelope 的当前时间。原始候选正文仍由既有 admission reconciliation 筛选，取消不能让未准入候选变成历史正文。

播放器的文字只是显示动画。遇到到期依赖事件，先补完前一 canonical 文字再按逻辑顺序应用事件，不延后取消或新输入。独立 notice 可穿插但不改变原消息归属。终端启用 bracketed paste 并忽略 Paste 事件。

## 验证原则

既检查最终 Markdown parity，也检查中间事件位置、工具未完成标记、旧 terminal 后的消息是否被切开，以及输入/source 无副作用。只读 fixture 固定追加发生时机，避免概率性并发测试。未执行的压力/端到端场景明确记为未验证。
