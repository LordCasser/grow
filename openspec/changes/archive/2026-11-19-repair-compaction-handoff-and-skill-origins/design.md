## Decisions

前台准备接手旧 generation 时使用现有 cancel-and-join 屏障，等待 Sideband 结算并确认旧事务终态，再尝试新事务。普通控制回调仍只发送取消信号。

保留在技能结算 replacement 中的 SkillLoad 继承原加载坐标。只有被结算的叶子从分支退出。实时、全量回放与 checkpoint 使用同一身份语义。

模型准入由当前请求投影和显式输出预留决定。自动预防压缩没有选区属于确定性的计划结果，同一 turn 抑制重复尝试。没有提交 replacement 的压缩事务仍保留原有失败记录；如果当前请求可以发送，客户端接收带 no_eligible_range 原因的压缩终态，正常结束进度提示。已知硬超窗、provider 拒绝、手动操作及持久化错误保留各自失败语义。

## Verification

构造失效 worker 与前台接管交错、混合已消费与待消费技能结算、软阈值及硬超窗的实际请求。对照原会话的只读事件和受影响 crate 的回归。

## Indexed resume

在现场副本上重建工作索引时，历史 ContextRecall 的校验将内存中保留的事件数量与冻结序号比较。源日志读取器已验证并略过纯诊断事件，因此两者可以不同。恢复使用父 Timeline 已验证的 canonical 序号裁切冻结分支，再按原 manifest 校验 revision、上下文坐标和可召回的历史叶子。已被旧技能结算提交的内容删除保留在历史中，恢复仅验证其确切变换。
