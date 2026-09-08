## ADDED Requirements

### Requirement: Shared application entry
grow CLI SHALL 根据命令和参数路由到交互界面、Agent 服务或 headless 执行入口。

#### Scenario: 无交互运行
- **WHEN** 用户提供 headless prompt 参数
- **THEN** 构造 HeadlessOptions 并进入 headless 路径；CLI 管理命令由各自分支处理。

证据：`crates/codegen/cli/src/main.rs` — `HeadlessOptions`。

### Requirement: Machine readable headless output
headless SHALL 按 OutputFormat 区分 JSON、StreamingJson 与 StreamingMessagesJson 输出。

#### Scenario: 流式机器读取
- **WHEN** 输出格式选择 streaming messages JSON
- **THEN** 通过对应事件封装输出；include_partial_messages 控制该格式的部分消息事件。

证据：`crates/codegen/pager/src/headless.rs` — `HeadlessEmitter`。

### Requirement: Session storage separation
会话存储 SHALL 区分 summary.json、updates.jsonl、timeline.jsonl 与 sidebands。

#### Scenario: 恢复本地会话
- **WHEN** 读取会话目录
- **THEN** 使用对应存储适配器读取摘要、展示更新、Timeline 及旁路账本，展示更新不替代 Timeline 事实。

证据：`crates/codegen/shell/src/session/storage/mod.rs` — `TIMELINE_FILE`。
