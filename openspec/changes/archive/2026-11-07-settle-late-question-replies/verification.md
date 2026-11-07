# 验证与父裁决

本调查未复现用户“原任务完成后迟答使主会话持续卡住”的报告。当前交付是已执行的反例调查与范围裁决，不声称该现场根因已修复。

## 执行证据

- `/tmp/grow-late-ask-discriminator.log`：5 passed / 0 failed，EXIT=0。问答相关三项分别验证长时间等待后正常单次结算并续下一问、原receiver关闭后迟到ACP答复惰性且worker继续、open/resolve ACK分别扣留及释放。另两项rg反控不能计为问答覆盖。
- `/tmp/grow-late-ask-worker-regress.log`：14 passed / 0 failed，EXIT=0，含blocking/Plan持久准入、cancel/suspend与freshattempt复用callid。任务记录保留最初测试编译/fixture修正，不把编译错误当行为旧红。
- 后续advisory特性另取得真实MvpAgent/ACP与新CLI的正常blocking、Settings强制blocking及冷恢复对照；它们只增强正常阻塞路径证据，不模拟原报告的所有transport时序，不能替代原场景复现。

父在本记录再次核两份实际日志中的测试名、count和exit；源码/UI/transport复核结论见[review](ui-transport-review.md)。

## 裁决

1. `InteractionResolved`已有durable resolve及guard drop两个发送路径，Pager按session+call关闭弹层并恢复草稿；不新增取消协议。
2. worker extfuture与Pager response sender间有两层oneshot。提交旧答时本地view仍会关闭/restore，silent send失败不能解释持续busy；不据此重做question代际协议。
3. 扣留ACK证明通用持久前提会阻塞，不证明生产在用户场景真的扣留ACK；unknown ACK不能凭超时被降为Failed。本轮不添加任意durable deadline或以失败解除已可能接纳的输入。
4. 新推荐继续需求采用独立行为change，Presented只表示展示，真实迟答用新stable FIFO输入；不能把该新功能宣称为原阻塞故障的已证修复。

调查范围已完成。原现场报告保留为证据门槛项：若再现，需拿到精确source session/call、原Turn结束原因以及同期transport/interaction/canonical状态，或通过真实duplex/reconnect控制得到同一持续卡住；届时按真正所有者单独修复。当前没有为猜测新增生产实体、接口或等待政策。归档只关闭本次调查交付，保留此未复现限制。
