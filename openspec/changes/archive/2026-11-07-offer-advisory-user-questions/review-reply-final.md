# U1/U2 最终输入接纳复核

原独立reviewer `01a1b1dd-872d-7991-a454-12de57f1ed16`在修复后只读复核最终输入链，未运行Cargo、写源或重审renderer。结论是原U1/U2已消除，限定范围无阻断发现。父随后亲读`completed_advisory_reply_observation`、`reconcile_advisory_reply`及`InputAdmissionStatusResolved`身份门闩，并核实际advisory日志，接受这一输入owner模块；U7和整项产品验收仍单独开放。

- U1：PromptResponse/durableTurnCompleted的赢家在drain之前，以已finalized prompt的完整稳定身份生成canonical query。另一rail迟到只合并或忽略；helper不把本地完成当Accepted，卡片Accepted由实际durable观察决定。
- U2：真实card Check/retry产生一次性open intent；只有同源durableUnknown/Absent、idle、current_prompt_id为空且有匹配冻结正文时，才将原immutable正文/ID释放到既有FIFO。自动Absent/transport error不释放review row，Submitted和terminal不授权重送。
- callback先核primary owner/session/bindingepoch、完整promptid和source=durable；没有response元数据的transport failure只允许冻结已绑定card。terminal setter在queue/inflight变更前拒绝stale降级。
- AdmissionReplay只退休同一次本地TurnSubmitting占位，不发假业务完成/usage/通知，也不清另一foreground。终态移除本reply retry障碍后，下项仍经普通drain guards；没有复活paused/stoppedGoal的路径。

`/tmp/grow-ask-editor-focus-green.log`实际37 passed/0 failed，包含U1/U2/U3/U5/U6与身份、child/primary、FIFO、Goal控制；`/tmp/grow-ask-focused-pty-product.log`实际2 passed/0 failed，证明freshCLI自然键盘主链与Settings。父没有为只读review重复跑同一矩阵。旧verification-reply-admission的十项未运行记录作为历史保留并加现状指引。

范围限制：这次不验证U7布局修复、mouse首次正文显示或全feature归档；前述freshCLI不是未来U7产物。最终产品验收仍必须重建无traceCLI并跑原mouse主链，不能沿用诊断二进制或加PageDown来通过。
