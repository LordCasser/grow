# Verification

## 场景与实现核对

五个新增 actor 命令级测试覆盖 required 未 Released 的拒绝、required Released 的允许、optional 未 Released/已 Released 的允许，以及 optional 活跃/未知 link 即使已有 Released 仍拒绝。成功切换不伪造旧 link terminal/result，失败不修改 Timeline 计划。actor 直接使用既有 `LocalPlan::validate_transition_from`，该规则继续拒绝等待及未处理 replan。

审查修正了初稿测试中变更 link.required 的非法夹具：改为初次创建时确定 required；首轮编译发现测试 helper 返回 LocalPlan 与声明 unit 不符，已修正映射。未放宽生产验证器或失败场景。

## 执行

使用 Rust/Cargo 1.93.1 和 `RUST_MIN_STACK=16777216`，与挂起修复联合定向验证；命令与结果见完成记录。没有修改 tool-types 格式或 link 身份语义。

## 完成记录

```sh
RUST_MIN_STACK=16777216 PATH="$HOME/.cargo/bin:$PATH" cargo test --locked -p shell --lib -- session::actor::suspension:: session::actor::tests::planning_dispatch_tests:: session::actor::tests::reverse_request_session_id_tests:: session::actor::tests::runtime_ agent::mvp_agent::tests::suspension_admission_tests:: --test-threads=1
```

**53 passed、0 failed、0 ignored**，日志 `/tmp/grow-resuspend-scope-regressions.log`。新增跨 release 矩阵为一个测试、内部 12 个组合；scope 为 5 个测试，不将组合数重复累计。归档前 strict OpenSpec：18 passed、0 failed。`git diff --check` 通过。

归档后当前规范 strict：16 passed、0 failed；archive strict：657 passed、0 failed。

联合收尾执行 `cargo clean -p shell -p tools`：移除 547 个文件、3.4 GiB 编译残留。
