## Context

Shell session actor 拥有唯一前台和用户 FIFO。Goal continuation 只在前台空闲且 FIFO 为空时启动；用户输入应先于 Goal 续跑。Pager 的本地 `pending_prompts` 与 Shell 的 `pending_inputs` 是不同阶段；前者在编辑队首时由 `drain_blocked` 暂缓，本次缺口主要在已获 Shell 接纳、由 `grow/queue/changed` 投影的队列项。

现状证据：

- `pager/src/app/agent_view/queue.rs` 已把队列项 `[edit]`／`[cancel]` 分别路由到编辑和删除；`queue_edit.rs` 在进入服务端项编辑时发送 `QueueHoldEdit`。
- `shell/src/session/actor/run_loop.rs` 将 hold 放入 `combine_edit_holds`；`notification_drain.rs` 只把该集合交给 combine，之后仍可提升队首。
- `shell/src/agent/mvp_agent/acp_agent.rs` 的队列通知白名单遗漏 `grow/queue/hold_edit` 与 `grow/queue/release_edit`；虽然 `ext_parsers.rs` 可解析它们，当前 Pager 发出的 hold/release 实际不会进入 actor。这是先于提升竞态的现存失效点。
- `pager/src/app/root/effects/mod.rs` 将 hold、release、edit、remove 作为无结果通知发送；编辑保存退出 UI 先于 Shell 替换结果。`prompt_queue.rs` 的替换虽然走新的 durable input admission，但失败对编辑器不可见。
- `docs/architecture/input-routing.md` 的仲裁顺序是用户 FIFO、notification、Goal。修复不得给 Goal 添加第二条用户输入队列。

## Decisions

1. **两个问题共享一个最小闭环，但验收分开。** Goal 下编辑／撤回是现有队列能力的可用性修复；“编辑期间不得发送”是新增行为。Shell 仍只维护一个 FIFO，Behavior 只影响执行时的上下文与后续续跑，不决定消息是否可编辑。
2. **hold 在权威队列项上确认，而不是靠 Pager 的视觉状态。** 进入服务端队列项编辑时，Pager 携带队列 id、版本和客户端身份请求 hold；Shell 在与队首提升共用的控制栅栏内检查该项仍待执行，再确认 hold。若已被提升、删除或版本变化，拒绝进入“已保护”编辑态，并保留用户现有 composer 内容。optimistic echo 尚未获 Shell 确认时不宣称可持有。
3. **复用现有临时 hold 概念，扩大其唯一职责。** Shell 的 hold 集合需使被持有项既不能参与 combine，也不能被提升；它不是持久 input fact。较早的未持有项仍可按 FIFO 执行；持有项一旦到队首，后项、notification 和 Goal continuation 都不能越过它。无需暂停 Goal 或取消当前 turn。
4. **编辑完成和解除 hold 具有确定顺序。** 保存时在同一控制边界完成新 payload、新 input_id 与旧 identity 的 durable 替换，成功后才解除 hold、广播版本并唤醒现有 idle arbiter；失败时旧项和 hold 保持不变，编辑稿留在 UI 并显示错误。放弃编辑则解除 hold，原文保留；撤回先 durably dismiss 输入，再删除该项并解除 hold，随后唤醒仲裁。若会话已空闲，解除后的那次仲裁可立即成为“下一个发出点”；若当前 turn 未结束，继续等其正常结束。
5. **不让过期释放解除别人的编辑。** hold 绑定发起客户端及一次编辑身份；重复 release 幂等，旧编辑的迟到 release 不能释放新编辑的 hold。客户端断开或编辑视图销毁时清理其临时 hold，避免 Goal 永久饥饿；原输入仍按 FIFO 留存。跨客户端的冲突以权威队列版本和明确失败结果处理，不用本地文本匹配。
6. **撤回只作用于未执行项。** `[cancel]`／队列键盘删除走版本化权威删除；Shell 若发现项已在运行或版本已变化，返回未撤回并同步最新队列。Pager 不把乐观隐藏当作成功。Ctrl+C 仍控制当前 turn／Goal，队列撤回入口属于所选队列项；不得偷偷把 Ctrl+C 改为两义操作。

### 控制协议与所有权

队列的 hold、save、release、remove 共用一个 `grow/queue/control` ACP request/response。请求携带 `sessionId`、操作、队列 `id`、`expectedVersion`，编辑操作另带一次编辑的随机 `editId`。成功响应给出操作结果和当前版本；失败明确区分队列项缺失／已运行、版本过期、被另一编辑持有和持久化失败。`grow/queue/changed` 仍只负责多客户端投影，不充当命令回执。旧的无结果 edit/remove/hold/release 通知入口在切换后移除，避免绕过版本和 hold 校验。

Leader 转发该 request 时覆盖写入其连接身份，Shell 以连接身份和 `editId` 一起拥有临时 hold；普通本地 ACP 连接的生命周期与 Agent/actor 相同。Leader 的客户端断开事件向 Shell 发送内部清理通知，actor 在控制栅栏下解除该连接的 hold 并唤醒 idle arbiter。旧编辑的 release 或迟到 save 只有身份和版本都相符才可改变队列；重复 release 在没有同身份 hold 时为幂等成功，不能解除另一持有者的 hold。

Pager 在 hold 回执前保留原 composer，不进入受保护编辑态；回执到达时还要核对当前 session binding、队列行版本和请求 `editId`。保存时保留编辑态和文本直到 durable 替换确认；失败保留文本与可继续操作的编辑态。撤回在成功回执或权威队列广播前不乐观删除行。编辑行被另一客户端移走时，Pager 保留已修改文本供用户显式处理，不把未保存文本悄悄恢复成旧 composer。

持有队首不能被其它路径消费：显式 send-now/interject 拒绝 held 行；reorder 不移动 held 行；clear 不删除 held 行。解除 hold 后只唤醒普通仲裁，不直接启动 Goal 或 turn。

## Risks / Trade-offs

- 用户可能在 hold 请求到达前遇到队首提升：以 Shell 栅栏内的确认／拒绝解决，不承诺已执行项可撤回。
- 保存前 UI 与 Shell 断连：不得展示“编辑成功”；重连后以权威队列／运行项重建，未确认草稿留在可恢复编辑位置或明确告知冲突。
- 多客户端同时操作：hold 身份及版本检查防止晚到结果覆盖较新操作；队列广播只负责投影，不代替命令结果。
- 持有队首会暂缓 Goal 续跑：这是用户正在编辑待执行输入的预期顺序；客户端退出要释放临时 hold。

## Validation Approach

先用 Shell 单元／集成测试重现“Goal turn 结束时队首正被编辑却被提升”；再验证 Normal 与 Goal 使用同一栅栏和 FIFO。Pager 测试覆盖 Goal Active 的 `[edit]`、`[cancel]`、键盘路径、失败反馈与草稿保留。最后用真实 ACP/PTY 场景验证完成编辑后恰在下一次正常发出点发送新文本一次，不停止 Goal 或当前 turn。
