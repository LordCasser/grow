# 实现交接与验收

父已核当前公共查询、fold验证及Shell恢复/编辑实际调用链。`ChatStateCommand::GetPendingAllowedInputs`直接返回`Timeline::pending_allowed_inputs()`；当前查询逐物理Submitted序过滤pending，尚未继承supersedes槽位。`AdmissionResolved`只允许替换尚未消费/终止/预留且已Allowed Fifo的输入，self/duplicate等仍由原validator拒绝。`InputFold`是派生私有状态，不是wire schema。Shell实际编辑的durable payload.prompt_id为`original:edit:N`，原live行只更新input_ids/blocks/meta，保留runtime prompt_id。冷恢复当前却直接把payload.prompt_id放回runtime，与live不一致。

## Task T · ChatState canonical pending order

只交Timeline/Input所有者的一项功能：既有pending查询中，成功接纳的替换继承所有被替换输入的最早有效FIFO槽位，连续编辑也继承；未替换输入沿原Submitted顺序。允许private fold派生位置或query局部计算，局部表示由coder选择；不增加持久事件、公共schema、Shell自行排序、第二份队列或新的公共查询。

写入限`chat-state/src/timeline.rs`及该模块既有测试、必要同owner actor公共查询测试；无其他业务owner适配。保留Allowed/Consumed/Handled/Blocked/Dismissed/Reserved/Steer与先prepare再accept等原规则，不用新的排序放宽validator。对多input合并行，取其最早有效槽位而不是supersedes列表顺序或最后Submitted。新的正常输入不冒领旧槽位。父核公共`PendingAllowedInput`返回结构保持。

证据要经过实际`Timeline::record`/validator和既有Actor handle公共查询，不直接注入fold；有效oldred A/B/C后编辑A，重建同一事件序列后查询，旧为B/C/A′，新为A′/B/C。连续A两次/B一次、supersedes逆序多input，以及消费/拒绝/未知接纳的原过滤使用必要控制或复用旧回归。每个successful durable cut恢复一次，不新增Hook/Submitted。局部Timeline测试可确认派生行为，Actor公共查询证明真实消费者拿到同序；本Task不冒称真实Shell编辑入口已通过。

## Task S · Shell restored row/runtime identity

先依赖Task T公共查询验收，后交Shell恢复所有者一项修复：有StoredQueueEntry时InputItem.prompt_id从其queue.id恢复；最新immutable payload正文、图片、client、版本和admission identity由原pending input id关联读取。无queue的普通输入保payload.prompt_id默认。F1 preservedTurn的Steer恢复路径保持，新增排序不在Shell实现。

写入限`session/actor/input_admission.rs`和现queueedit/recovery测试；同一次真实编辑入口产生A/B/C→A两次/B一次，持久日志切点销毁重建owner，核顺序、原行/运行id、最新内容/图片/版本、旧inputRouteSuperseded、一次消费和Hook不重放。需要unknownACK控制时沿现durable harness，不把failedACK当未写，也不更改F3的immutable conflict语义。拒绝保存保原行/hold；手动reorder重启持久化是另一政策，本项不承诺。

## Review Plan

父分别接受T公共顺序契约与S恢复身份实现，再核同artifact的跨层cold恢复证据，运行现F1/F3及queueedit相邻回归。两项各自实际编译/定向测试通过后才勾tasks。最终docs/backlog/strict+archive与本change场景逐项对应；未运行的平台和故障窗口如实保留。Cargo与Rust源稳定窗口按唯一owner串行，不能在上一executor尚未释放时写依赖源码。
