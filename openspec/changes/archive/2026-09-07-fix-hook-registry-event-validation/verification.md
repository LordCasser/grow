## 失败证据
旧实现 targeted restored_registry_：0 passed / 2 failed。事件 map 键 SessionStart 内含 PreToolUse 仍返回 Ok；SessionStart on_failure=block 也返回 Ok。

## 修复与验证
- 反序列化逐个验证事件身份及已有 HookSpec.validate，任一错误拒绝整个快照。
- 事件矩阵覆盖 ALL 中每个事件的合法往返、不同事件篡改；完整多事件快照保留组内两个不同名称的 handler 顺序。
- 最终 hooks：228 单元 + 13 集成 + 1 doctest 全通过。
- workspace hook_：13 通过，覆盖 wire 往返和实际 matcher 行为。
- rustfmt 与 git diff --check 通过。

命令使用 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline。

## 边界
未读取或修改用户会话文件；此变更只验证 registry 恢复身份及已有失败策略约束，不扩大 HookSpec.validate 的职责。
