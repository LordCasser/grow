# 验证

2026-10-31，macOS，Rust 1.93.1。沿用现有真实 ChatState 与 HTTP sampler mock；ACK fixture 控制确认顺序，不模拟断电。

## 原实现失败

`cargo test --locked -p shell --lib suspension_retires_foreground_after_compaction_drains -- --test-threads=1` 失败：放行压缩取消终态 ACK 后，挂起仍返回 `active compaction owner did not settle before the suspension deadline`。前台使用真实 handle_prompt，stopped sender 与该任务同生命周期，不使用独立 sleeper 的 stopped。

初版 Summary ACK held 夹具阻塞了挂起的 ChatState 查询，已修正为在摘要请求进行时启动挂起，随后截住 Failed ACK；初版夹具超时不是根因证据。

## 修复后

`cargo test --locked -p shell --lib suspension_ -- --test-threads=1`：17 passed、0 failed、1 ignored。ignored 是既有本地长历史测量，不计入通过。

- 新增正常排空回归：ACK 未确认时 lease 与真实 foreground 仍在；ACK 放行后同一次挂起完成，前台被物理中止并 join，没有 TurnEnded，也没有后继 provider 请求。
- 新增超时重试回归：Failed ACK 超过 deadline 时返回未确认且原任务仍在；放行 ACK 后 lease 释放而 foreground 停在真实 handoff park，同 operation 重试成功。
- 新增 lease 边界回归：关闭期间拒绝新 owner；两个异步 idle 等待者都被释放唤醒；guard drop 保留关闭位；恢复后才允许取得 lease。

另从 Cargo 产物枚举并逐项 `--exact --test-threads=1` 验证 52 项（每项检查 `1 passed; 0 failed; 0 ignored`）：suspension 模块、runtime restore、runtime suspended waiter、compaction admission、lease/cancel gate，以及 18 项异步压缩回归。与上述 17 项重叠 9 项，本次合计 60 个不同 Shell 测试通过。

构建环境：`PATH="$HOME/.cargo/bin:$PATH" CARGO_BUILD_JOBS=2 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_DEV_DEBUG=0 RUST_MIN_STACK=16777216`。

定向 rustfmt 与 git diff --check 通过。只读对抗性审查核对 closed-bit CAS、Drop 保留关闭位、Notify 注册时序、迟到手动 worker、Sideband/Goal 结算边界与 restore reopen，未发现阻塞项。

OpenSpec 全量严格校验在归档前、归档后均通过，archived tasks 校验通过。本次独立 change 已归档，backlog 对应待复现项转为已解决记录。验证完成后清理本次 Shell 测试可执行文件，保留共享依赖缓存。

临时输出保存在 `/tmp/grow-suspension-red.log`、`/tmp/grow-suspension-green.log`、`/tmp/grow-suspend-regression.log`。不声称运行完整 Shell 测试包、长历史性能测试或真实磁盘故障测试。
