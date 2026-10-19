# Verification

## 发布失败基线

- [首次 v2.4.0 发布 run 39191289841](https://github.com/LordCasser/grow/actions/runs/39191289841)，commit `966775374078c162dc6fa7e557ecfa0d3f416f54`。
- 最终 10 个资产构建中 6 个成功；Linux aarch64、Windows x86_64/aarch64、macOS aarch64 四个在 `test_local_coordination.py::is_queued` 收到 `source_unavailable` 失败。Linux x86_64 和 macOS x86_64 的同一 smoke 没有复现，不能按硬件归因。发布 job skipped，GitHub 未创建 v2.4.0 Release。
- 原 annotated tag object 为 `60c7002313edde0769cba0a08588ecfbc1ac7b01`，解析到上述 commit；本节记录失败基线，不声称其为修复后产物。

## 可控时序 RED → GREEN

- 先为 runtime 增加接纳 future 入口，临时保持生产 backend 原来的“await admission → runtime ask”顺序；oneshot 确认真实 JSONL `OutgoingStarted` 已落盘但 ACK 被阻塞后查询同一身份。
- 初次编译命令沿用了旧 filter 名并带 `--exact`，结果 0 tests，仅用于完成编译，**不计为验证通过**。随后直接执行新构建 test binary 的 `query_observes_live_source_startup_before_durable_ack`：**1 failed**，结构化错误为同一 `SourceUnavailable`，日志 `/tmp/grow-inquiry-startup-red-exact.log`。不需要真实进程崩溃或 persistence actor 故障即可复现。
- 修复后运行 `cargo test --locked -p shell --lib coordination:: -- --test-threads=4`，首次及最终均 **48 passed / 0 failed**。最终日志 `/tmp/grow-inquiry-startup-green-final.log`。
- 三个新增测试：`query_observes_live_source_startup_before_durable_ack`、`cancellation_during_source_admission_never_dispatches`、`failed_source_admission_does_not_dispatch_or_leave_a_live_record`。屏障期间确认 Discovering、无 outcome、目标无派发；同身份重试通过 progress channel 确认在 ACK 前已经加入且只执行一次 admission；冲突 callback 不被调用；取消 RPC 在 ACK 前完成；失败移除登记，显式重试再次进入 admission。
- 既有 `cold_source_started_fact_pins_target_and_question_without_a_terminal` 等恢复/身份/协议测试继续通过，历史 pending start 不被映射为活执行或自动重发。

## Shell 与实际 CLI

- 使用 rustup 的 Rust/Rustdoc **1.93.1**、`CARGO_BUILD_JOBS=2`、`RUST_MIN_STACK=16777216`、禁用 incremental/dev/test debug，从当前源码重建。Homebrew 的 cargo 不解析 `+toolchain`；格式化改用 rustup cargo。
- 在隔离临时 `GROW_HOME` 执行当前 Shell test binary 全量：**4254 passed / 0 failed / 5 ignored**，耗时 123.35 秒，日志 `/tmp/grow-inquiry-startup-shell-all.log`。此后仅加强新测试中的 retry progress barrier 和失败后重试断言，最终 48 项协调测试已重新编译并运行；产品实现未再变更。
- `cargo build --locked -p cli --bin grow` 成功，日志 `/tmp/grow-inquiry-startup-cli-build.log`。
- `python3 scripts/test_local_coordination.py --binary target/debug/grow` **exit 0，全部 10 个 PASS 场景**，日志 `/tmp/grow-inquiry-startup-process.log`。包含原 FIFO queued 查询失败场景、真实工具路径、取消与一次权限决定、持久化对称审计、source/target 重启、resident reload、running/queued crash receipt；原脚本断言未放宽。
- 以上为本机 macOS arm64 的当前源码构建，不冒充远端发布 binary 或其他平台通过；全部模型访问为脚本的隔离 loopback fixture，没有真实 provider 互操作结论。

## CI、文档与格式

- `Local agent coordination` 新增相关路径的 `push` / `main` trigger。Ruby YAML 解析及结构检查通过：push/PR paths 完全一致，原真实进程步骤保留；push 事件没有 process_only 输入，库测试继续执行。
- 更新开发说明及 2.4.0 发布说明；[audit.md](audit.md) 保留测试分层、漏跑进程脚本、缺失 main push trigger 及清理日志误归因的复盘。
- `cargo +1.93.1 fmt --all --check`（通过 rustup cargo）、`git diff --check` 均通过。
- 归档前 `openspec validate --all --strict --no-interactive`：**17 passed / 0 failed**，日志 `/tmp/grow-inquiry-startup-openspec-before.log`。

## 审查与归档

root 对周边调用及失败路径核对后，Luna 对指定 runtime/backend/test/workflow diff 进行了独立只读对抗性复核，未发现阻断缺陷；没有将其审查计为再次执行测试。重点核对 pending ACK 不持 map 锁、取消 RPC 可命中、失败释放、同身份去重、cold source 不重跑和 Result channel 各消费路径。

query 初次未命中到 durable fallback 的第二窗口由互斥顺序闭合；本次没有另加专门控制磁盘读取交错的测试，不将代码审查当作该场景的独立动态证明。新屏障测试动态覆盖的是本次实际失败的 durable-start/ACK/register 窗口。

完成归档后：`openspec validate --all --strict --no-interactive` 为 **16 passed / 0 failed**，`openspec validate --archived --no-interactive` 为 **609 passed / 0 failed**。开发说明已改为链接主规范中新归档的 requirement。
