## 1. Simple English TUI and Markdown

- [x] 1.1 对照当前 main specs、相关并行 changes、真实 tool/notice/viewer 风格，固定三类入口和回归基线；不修改无关 Rust 工作树。
- [x] 1.2 按 tui-design.md 精简英文固定文案，去掉常驻路由/投递模式解释和每行详情按钮，保留真实身份/状态/正文、原位更新与既有 footer/keymap。
- [x] 1.3 三类通信正文分区接入 MarkdownContent，支持 Q2/Q1+A2/M2/错误预算、代码/表格/Mermaid；typed 原文保留 tab/CRLF；详情 raw/Data 与选择复制走真实入口。
- [x] 1.4 验证 Normal/Minimal/replay、40/60/100 列、GrowNight/GrowDay/无色、resize、同长度正文替换、选区中终态到达、英文固定文案与非英文原文保真。

## 2. Receipt contract

- [x] 2.1 发送分支透传真实 receipt ID，收紧 received/rejected/unconfirmed outcome 和 typed error，移除当前 UI 基于错误文字的语义判断；历史缺字段只在读取边界处理。
- [x] 2.2 区分新消息 admission 与已有 receipt 只读核验，保留 ownership/active gate，不开放新 peer 写权限；验证 ACK/取消竞争、commit 前后故障、同操作去重/冲突、未知不重发。
- [x] 2.3 验证 source 工具结果保存失败、inactive target 回执核验、冷恢复 UI 重建、Minimal 一次输出及双端 Received/Consumed/provider request 边界。

## 3. Opinion exchange

- [x] 3.1 在现有 inbox 消费中接入 runtime agent-message context item，保留 receipt/source/target/reply_to/body；原子提交消费与输入，不重做所有通知类型。
- [x] 3.2 在 provider-neutral projection 生成稳定的专用收件工具调用/结果对，验证所有现有 endpoint、portable history、compaction/input_ref、provider 切换和统计；不将 runtime 收件登记为模型主动工具执行。
- [x] 3.3 实现沿已收到消息的反向 reply 路由，runtime 校验双方与原关系；明确 idle/busy/paused/closed admission，禁止上行 interrupt、越权或因回复自动恢复关闭任务。
- [x] 3.4 通过双 agent 真实请求验证双方都保留来往意见、无 ACK 循环/互等/重复消费；ask Sideband 不写目标主 Surface，显式 send 引用后才成为正式消息。

## 4. Documentation and closure

- [x] 4.1 同步 docs/architecture/local-coordination.md 与相关请求投影说明，运行受影响 shell/chat-state/sampling-types/pager 定向测试，逐条核对 delta 场景并记录未执行项。
- [x] 4.2 全部实现与运行时验证完成后运行 openspec validate --all --strict --no-interactive，归档并验证 archive。
