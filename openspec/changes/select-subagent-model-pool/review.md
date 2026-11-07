# 子 Agent 模型池：待用户同意的方案

输入 `/subagent-models`，打开与现有扩展面板相同风格的选择窗。按provider分组，可搜索；Space勾选，Enter应用，Esc放弃本次编辑。

```text
 Subagent models                         Search: …

 Current primary model
   ● current/model                 always inherited

 Provider A
   ☑ provider-a/model-a
   ☐ provider-a/model-b
 Provider B
   ☑ provider-b/model-c

 Additional models: 2
 Effort is chosen by the Agent for the selected model.

                    Enter Apply     Esc Cancel
```

池为空时只允许当前主模型。当前主模型是固定继承行，不必手工勾选；其他勾选是允许主Agent自主选用的额外模型。没有effort选择器，主Agent可选目标模型实际支持的effort。不可用目录项保留显示，但不授予调用资格。

这版作用于当前主会话及其子任务树，恢复该会话后保留，普通fork复制后独立。它不改其他会话的默认模型。正在运行的子任务可以按已准入模型结束；新派发、显式模型切换和新resume按当前池校验，子Agent不能扩大根池。Workflow冻结路由沿原规则。

主Agent会在同一个Step边界收到三处一致变化：工具说明列出完整可用模型及effort规则；下一次正常推理前加入明确的系统提醒，说明旧列表已被本版替代；实际工具调用按同一版本检查范围。池状态和提醒一起持久化，重连和压缩恢复当前版本。单纯修改空闲会话的池不会额外发起一次付费推理。

例如正在生成时把额外模型从A改成B，面板先显示待应用。本Step关边界后生效，下一次请求看到“当前主模型+B”；已经准入的A子任务继续，之后再申请A会收到带当前可用列表的拒绝。若保存失败，仍按旧池执行；两个客户端用旧版本覆盖新版本的提交会明确失败并刷新当前状态。

方案只建立候选规范和设计，尚未实现产品。完整职责、原子生效、恢复和验收条件见 [design.md](design.md)，实现前按用户明确要求取得同意。
