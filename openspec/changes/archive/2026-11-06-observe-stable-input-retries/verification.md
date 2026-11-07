# Verification

父核对最终canonical查询、Shell接纳、队列应答和ACP短路链。独立review `01a1af56-cba6-7d72-8daf-22829fe2e744` 的queue-edit replay假成功发现已修：只有内存行的input_ids包含已接纳edit identity才返回成功；尚未应用replacement沿既有queue_changed_after_admission错误退出。提供方SubmitInputDurably已有精确input_id/intent/payload_ref约束，不增加违约fake驱动的重复防护。F2的编辑顺序/身份仍为独立后续项。

`/tmp/grow-backlog-shell-integrated-final.log` 的统一 **96 passed / 0 failed、exit0** 包括输入接纳、edited-save replay、canonical Blocked/Handled/Consumed 等反例，以及相邻child/F4回归。`/tmp/grow-backlog-input-fixtures-final.log` 两项定向回归也通过。此前空Hook被错误构造为Block、edit identity与真实version算法不符均修正fixture，没有放宽Timeline validator或stale version保护。

真实CLI ACP验收位于 `shell/tests/test_input_admission_replay_e2e.rs`。最终 `/tmp/grow-backlog-input-replay-e2e-final.log` **1 passed / 0 failed、真实exit0**。fixture用当前provider配置、per-expectation foreground terminal barrier，并先观察真实Working；原prompt RPC保持pending时，同promptId/原payload重试返回原inputId及与物理Timeline一致的状态。变更正文对照返回identity conflict；重试/冲突窗口均无新增Submitted、Hook、Turn或provider请求，无新的本session roster delta，观察应答无usage/totalTokens/structuredOutput。释放唯一原请求后正常完成，终态重试仍为Consumed；真实Idle为通知流活性阳性对照。元数据用准确`_meta`，TurnEnded按Started的turn identity关联，完整Timeline行解析失败不会静默丢弃。

此前ACP编译失败（read_line双层Result、借用clone寿命）和mock前提失败（错误provider配置、未解包当前ext wire）不能作为产品旧红；最终fixture对齐实际入口后通过。不伪称复现旧用户会话或Windows现场；本项不包含F1挂起Steer恢复或F2多次编辑FIFO冷恢复。
