# macOS 测试线程栈边缘溢出：诊断与修复模式

**症状**：`cargo test -p shell --lib` 下某些 `#[tokio::test(flavor = "current_thread")]`
测试报 `thread '...' has overflowed its stack` + `fatal runtime error: stack overflow, aborting`，
**进程直接 abort**（后续测试全部不跑，`cargo test` 输出停在第一个溢出的测试）。

已确认实例（2026-08，merge-upstream 工作区）：

- `cancel_running_task_tests::persist_ack_waits_for_disk_flush_before_success`
- `chat_history_integrity_tests::mid_turn_user_injection_must_not_duplicate_tool_results_for_one_tool_use_id`

## 根因

macOS 上 libtest 的测试线程**默认栈约 2.03MB**（实测区域
`[0x16fe04000-0x17000c000)`，即 0x208000 字节，远小于 Linux 上的 8MB 默认），
而**深链测试**（完整 turn 处理 → `maybe_compact_on_model_switch` →
`with_resolved_model` → `load_effective_config` →
`new_from_toml_cfg` → TOML/JSON 解析）在 **debug 构建（无优化）** 下的栈需求约
**2.04-2.4MB**——刚好超出 2.03MB。async 状态机帧在 debug 下可以很大
（单帧 50-430KB 很常见），所以这是"余量仅几百字节"的边缘场景：
任何一次代码改动（多一个跨 await 变量、多一层调用）都可能把需求推过 2.03MB。

关键事实：

- 显式 `RUST_MIN_STACK=8388608`（8MB）时测试线程栈被抬到 8MB → 测试通过；
  不设置时用默认 2.03MB → 溢出。**这是最快速的判别手段**。
- 同一个测试在历史基线（HEAD）上"勉强通过"（余量 ~100B），合入上游改动后
  只增加了约 384 字节的栈需求就溢出。**不要假设"HEAD 能过 = 永远安全"**。
- 调试插桩（`eprintln!`、包装函数、asm SP probe）本身会增加栈需求，可能把
  边缘测试从"通过"推成"溢出"；**插桩恢复后可能自动消失**，别急着改逻辑。

## 诊断步骤（按顺序）

1. **确认是栈问题而非逻辑问题**：

   ```bash
   RUST_MIN_STACK=8388608 cargo test -p <crate> --lib <test> -- --exact   # 通过?
   cargo test -p <crate> --lib <test> -- --exact                         # 溢出?
   ```
   前者过、后者崩 → 栈边缘问题。两者都崩 → 真递归/无限递归，另查。

2. **确认测试线程的实际栈范围**（lldb，崩溃时）：

   ```bash
   lldb -b --no-lldbinit -s script.lldb -- ./target/debug/deps/shell-<hash> '<test>' --exact
   # script.lldb: run → memory region $sp
   ```
   `memory region $sp` 显示栈区域与 guard page，可算出真实栈大小
   （macOS 实测 0x208000 ≈ 2.03MB）。

3. **定位链上的巨帧**（可选）：断点打到最深函数（如 `key_value` / 目标递归点），
   逐帧 `register read sp` 求相邻帧差；或 SP probe（arm64 用
   `core::arch::asm!("mov {}, sp", out(reg) sp)`，**注意不是 x86 的 `rsp`**）。

4. **逐个修**：`cargo test --lib` 在第一个溢出测试处 abort，**一次只能看到第一个
   崩的测试**；修完一个必须重跑全量找下一个。

## 修复模式

给深链测试显式大栈线程（**只改测试执行环境，不动被测逻辑**）：

```rust
// 原:
// #[tokio::test(flavor = "current_thread")]
// async fn foo() {
//     let local = tokio::task::LocalSet::new();
//     local.run_until(async { ... }).await;
// }

#[test]
fn foo() {
    std::thread::Builder::new()
        .name("foo".into())
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let local = tokio::task::LocalSet::new();
                    local.run_until(async { ... }).await;
                })
        })
        .unwrap()
        .join()
        .unwrap();
}
```

要点：

- 保留 `LocalSet` + `run_until` 结构（`spawn_local` 依赖它）。
- 删除残留的 `async fn` 行，body 整体 +8 空格缩进。
- 不要用 `RUST_MIN_STACK` 环境变量当修复（CI/其他机器不会设置）。
- 不要为了省栈去动被测逻辑（改 turn 链/配置加载的跨 await 变量属于
  架构级变更，需单独评审）。

## 其他注意事项

- 共享 `CARGO_TARGET_DIR` 时，**不同 worktree（不同源码版本）编译同一包会互相
  覆盖 fingerprint**，导致"源码 A 编译、源码 B 链接到 A 的 rlib"（症状如
  `cannot find function max_wait_block_ms in crate tool_types`）。此时
  `cargo clean -p <被污染包>`（必要时清所有 `grow-*`/`grow-*`）后重编。
- 深链测试的候选：任何走完整 prompt turn（含 model auth / 配置加载）的
  `#[tokio::test(flavor = "current_thread")]` 测试。上游合入后优先跑
  `shell --lib` 全量确认。
