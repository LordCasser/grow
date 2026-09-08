# OpenSpec 迁移复验 · 2026-09-07

## 结论

核心规范未发现与本次复查实现相矛盾的要求。修正三处依赖未跟踪文件的历史索引链接，并将文档工作隔离到 codex/openspec-sdd 工作树。规范仍是核心能力基线，不是全仓库行为穷举。

## 文档与过程检查

- 14 份规范严格校验通过；41 项要求的代码路径和符号均存在。
- 14 份主规范 Requirement 正文与迁移归档 delta 一致。
- 原工作树的 41 处证据引用对应历史源码哈希无漂移。
- 按已跟踪文件加迁移文件的独立交付清单检查，110 个本地链接及标题锚点有效；不依赖三个本地未跟踪审计文件。
- 57 个 Cargo workspace 成员均已映射，5 份 legacy 快照与原 docs 字节一致。
- OpenSpec config 和 CI 使用 YAML parser 解析通过。CI 的 PR 触发、只读权限、Node 22、固定 CLI 1.11.0 与两条校验命令均已核对；远端 Actions 未运行。
- 新建 skip_specs: true 的文档 change 后，validate 通过，instructions apply 返回 ready；按真实归档路径复验。

## 代码语义复查

复读 Timeline 持久化及恢复测试、工具协议修复测试；检查 Goal 的状态与阻塞计数、权限 deny 快速路径、Workflow 的发布 hash 与 Run source、journal 回放校验、MCP/Hook 计划、采样取消、Memory 降级和结果上限。Sandbox 的拒绝启动行为额外核对 shell/src/config/mod.rs 的调用方，避免只靠 helper 声明推断宿主行为。

Atlas 本次 Journal::replay 返回的源码范围与磁盘行号不一致，未把该摘录作为结论依据；直接读取磁盘实现，确认 seq/kind/req_hash 分歧返回 Divergence。

## Rust 测试

命令：`cargo test --locked -p chat-state -p workflow --lib`。
执行位置：迁移原始共同工作树 `/Users/lordcasser/workspace/projects/grow`，当时分支为 main，包含其他任务的未提交功能修改。

- chat-state：461 passed，0 failed。
- workflow：65 passed，0 failed。
- 合计：526 passed，0 failed；退出码 0。

测试曾等待另一项 Cargo 构建释放锁，之后正常完成。没有清理 target 或中断其他任务。此结果证明当时共同工作树的这两个 crate 测试通过，不冒充独立分支、全仓库或所有平台的测试结果。

## 独立分支复核

文档交付位置：`/Users/lordcasser/workspace/projects/grow-openspec-sdd`，分支 `codex/openspec-sdd`，代码 HEAD 与迁移起点一致。

独立分支的 41 处证据符号均存在；与历史工作树相比有三份源码哈希不同：memory/search.rs 只涉及测试调用签名，memory/storage.rs 涉及 classify_source 与枚举范围，goal_tracker.rs 涉及停止状态的耗时计量。本次规范涉及的搜索降级、构造目录和 Goal 状态/阻塞/预算要求在两边实现一致。差异保留在独立 evidence.json，不重写原历史证据。

并发审计的 audit-goal-settlement-retry 活动/归档记录均未纳入独立文档工作树。共同目录的迁移原副本保留，未删除或提交。

## 最终归档检查

本次文档 change 已成功按 skip_specs 归档。独立工作树最终校验：14 份规范通过，2 份归档通过，111 个本地链接及锚点通过，当前 evidence.json 哈希一致；没有未归档的文档 change。
