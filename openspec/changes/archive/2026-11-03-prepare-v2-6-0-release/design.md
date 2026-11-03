## Scope and authority

openspec/specs 是行为权威，源码、调用方与测试用于核对实现；归档记录只用于追溯本轮变化。审查从 v2.5.5/当前 HEAD 到候选工作树，覆盖用户可见接口、prompt 渲染、会话/子任务恢复和发布门禁。已明确暂缓的 backlog 不扩入本轮。

## Release preparation

用户确认版本 2.6.0。版本由根 Cargo.toml 的 workspace.package.version 驱动，Cargo.lock 同步；release notes 沿用 crates/codegen/shell/changelogs。提示与指南必须使用当前实际注册的工具名，区分 command 输出、child 清单、状态和等待结果，保留观察失败不等于完成/清理的既有语义。

使用 rust-toolchain.toml 指定的 Rust 1.93.1。先运行静态规范、格式和脚本验证，再执行受影响库、真实入口与生产 CLI 构建。发布绑定 tag 的 clean-tree/immutable-SHA 门禁只能在最终提交和 tag 后执行，不能以 dirty 本地产物声称正式发布通过。
