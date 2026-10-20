# 验证记录

## 隔离复现

复现源为 `reproduction.rs`。临时在 `crates/codegen/shell/src/session/actor/suspension.rs` 最后加入一个 `#[cfg(test)] mod audit_suspension_reproduction`，通过 `include!` 引入该文件，运行后移除。若从归档重新运行，应使用当前归档目录路径。

运行命令：

```sh
cargo test --locked -p shell --lib audit_suspension_reproduction -- --nocapture --test-threads=1
```

最终结果：2 passed，0 failed，4298 filtered out，测试执行 1.22 秒。这里的 passed 表示成功观测并断言当前缺陷，**不表示产品路径通过验收**。

```text
audit receipt conflict attempt 1: timeline event violates the causal fold: invalid notification fact
audit receipt conflict attempt 2: timeline event violates the causal fold: invalid notification fact
audit permission first=suspension owners settled after the deadline; retry the same operation identity; retry=Ok(RuntimeNodeRef { session_id: "test-actor", checkpoint_seq: EventSeq(12) })
audit permission result: Session suspended this operation (OutcomeUnknown). It may have made partial changes. Inspect the current state before deciding on a new operation; this call will not be automatically replayed.
audit permission checkpoint operations: [SuspendedOperation { call_id: "audit-permission-call", state: OutcomeUnknown, dependency_id: None }]
```

两次夹具编译问题（gateway 字段路径、测试 closure 的 `'static` bound）已修正后才运行成功。编译仍有仓库已有 `fetch_update` deprecation 和 linker `__eh_frame` 警告。

F1 从生产 formatter 生成正文，经真实通知接纳器写入，再预置 production terminal 已证实会产生的 CommandsSettled，调用完整 `suspend_node` 两次。F2 用真实 PendingInteractionGuard、前台 task abort、辅助活动租约和 deadline 调用完整 `suspend_node` 两次；不调用真实模型、权限服务或命令。它们使用现有 ChatState 持久化测试夹具，不能代替真实磁盘故障测试。

## 静态核对

- Luna 子 agent 枚举通知 source/version 配对，额外对抗检查 F1 的 completion ACK、terminal 留存和挂起枚举次序。
- 主 agent 核对人工等待 guard、权限 observer、ToolStarted 次序、挂起 cursor 重算、Timeline checkpoint 校验与执行型恢复入口。
- 未把 Atlas 空结果当成不存在调用方：两次 scoped search 都返回 partial / closure_boundary。
- 未对真实用户会话执行故障注入，未安装或替换全局 Grow 二进制。本次生产代码差异应仅保留上一轮的 Ordinal 修复。

## 收尾

- 临时 include 接线已移除；生产文件只保留上一轮 `Opaque → Ordinal(1)` 修正。
- `cargo clean -p shell` 清理本次测试编译残留 3.7 GiB；先前 `target/debug/grow` 保留且版本检查正常。
- 归档前 `openspec validate --all --strict --no-interactive`：17 passed，0 failed。
- `openspec archive audit-suspension-retry-boundaries --skip-specs --yes`：完成，未修改主规范。归档时收尾任务尚未勾选，第一次 archive 校验因此报告 1 incomplete task；记录后勾选并重新验证。
- 归档后全量规范校验：16 passed，0 failed；最终归档校验：650 passed，0 failed。
- `git diff --check`：通过。
