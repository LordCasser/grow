# 验证记录

## 执行环境与最终结果

Rust/Cargo 1.93.1，`RUST_MIN_STACK=16777216`，本地隔离 fixture。命令：

```sh
RUST_MIN_STACK=16777216 PATH="$HOME/.cargo/bin:$PATH" cargo test --locked -p shell --lib -- \
  audit_230_240 audit_released_undispatched \
  session::actor::tests::planning_dispatch_tests:: \
  session::actor::tests::mcp_catalog_lifecycle_tests:: \
  session::actor::tests::skill_loading_tests:: \
  session::storage::communication_history::tests:: \
  coordination:: --test-threads=1 --nocapture
```

**93 passed、0 failed、1 ignored**。其中 2 个是断言缺陷存在的审计探针，PASS 表示反例复现成功；其余 91 项是既有回归。ignored 是需要显式选择用户来源会话的只读检查，本次没有接触该真实来源。日志 `/tmp/grow-230-240-regressions-final.log`。分组：本地协调 48、MCP catalog 5、规划 14 + scope 探针 1、Skill 14、通信历史 10 + ignored 1、再次挂起探针 1。

A1 一个测试覆盖 Permission、Question、PlanApproval 三种等待，均在第二轮 checkpoint 观察到 OutcomeUnknown 与新工具结果。A2 验证 actor 与 LocalPlan 同一已释放未派发 link 的 scope 判定不一致。完整源码保留为 `resuspend_probe.rs`、`planning_scope_probe.rs`，临时模块入口已移除，没有留下断言缺陷的常驻测试。

## 中间失败的解释

- 第一轮 resuspend fixture 未恢复持久 Control revision，第二次 checkpoint 因 revision 1 不大于 2 被拒绝。按生产冷加载路径恢复最新 revision 与 Behavior 后重跑；这次夹具错误不作为产品发现。
- 第二轮已复现两条审计反例，但既有 Skill pipeline 测试在默认 Rust 测试线程栈上 stack overflow。按仓库既有验证配置改用 16 MiB 栈后，最终整组通过。不能把中途已通过的部分日志当成该次整组成功。

## 版本与证据强度

版本身份、提交范围和变更统计用本地 Git tag 核实。两个缺陷相关逻辑由 `2e69e5db` 引入，并在 v2.4.0 中存在；本次只在当前工作区执行探针，没有构建旧 tag 来宣称历史二进制实测。A1 使用明确启动信号屏障模拟 scheduler 窗口；A2 使用真实 Planning 命令与持久 Timeline。没有运行 OS 进程重启、真实 provider 或完整跨平台发布矩阵。

## 收尾

仅新增审计材料和 backlog。之前的挂起/Goal 修复及其他工作区内容保留；未安装二进制、未操作原故障 session。严格校验与清理结果见下方完成记录。

## 完成记录

归档前 strict：17 passed、0 failed；归档后当前规范 strict：16 passed、0 failed；archive strict：655 passed、0 failed。`git diff --check` 通过。归档提示 proposal Why 文字过短，已补充具体审计动机。

审计收尾执行 `cargo clean -p shell -p tools`，移除 807 个文件、5.9 GiB 编译残留。
