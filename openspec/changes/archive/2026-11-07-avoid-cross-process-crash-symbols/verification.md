# PB-06 verification: raw-only address policy

## 实施内容
- `crates/codegen/crash-handler/src/symbolicate.rs`：`resolve_frames` 对每个原 IP 逐字返回 `ResolvedFrame{ ip, symbol_name: None, filename: None, lineno: None }`，不调用任何 resolver；模块/函数文档改述 provenance 限制（pid/version 相等不构成地址空间授权）。`format_report` 在 Backtrace 段前输出固定说明：raw frames 来自 previous process、未捕获 module identity/load addresses、automatic symbolication unavailable、no function attribution。
- `src/lib.rs`：`CrashReport::backtrace` 文档不再声称符号化。
- `README.md`：`resolves IPs to symbols via backtrace` 改为 raw 保真 + 不可自动符号化表述。
- `Cargo.toml`：删除本 crate 唯一 `backtrace` 依赖（`rg 'backtrace::'` 全 crate 零匹配，生产零消费者）。
- `Cargo.lock`：单次 `cargo check --offline -p crash-handler` 收束；diff 仅从 crash-handler 包条目移除 `"backtrace"`，无任何包版本/日期变化；根 manifest 未动。

## 旧红（生产体未改 + 新测试，证明旧 resolver 确实错误归因）
- env：rustc 1.93.1、`CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216`、`--locked --offline`。
- `cargo test --locked --offline -p crash-handler --lib symbolicate::tests -- --test-threads=2` → exit 101，FAILED. 2 passed; 2 failed。失败即核心断言（非编译问题）：
  - `resolve_frames_preserves_raw_addresses_without_attribution`：`unverifiable address must not be attributed to the current map, got Some("{closure#0}")`
  - `format_report_states_symbolication_unavailable_for_raw_captures`
- `cargo test --locked --offline -p crash-handler --test integration recovery_does_not_attribute -- --test-threads=1` → exit 101，`recovery_does_not_attribute_current_process_addresses` FAILED，同一归因断言 `Some("{closure#0}")`。
- 日志：`/tmp/pb06/logs/oldred.log`

## 新绿（raw-only 生产体 + lib.rs 注释同步，同 filters）
- lib filter → exit 0，4 passed; 0 failed。日志：`/tmp/pb06/logs/newgreen.log`
- `cargo test --locked --offline -p crash-handler --test integration -- recovery_does_not_attribute cross_process_capture --test-threads=1` → exit 0，2 passed; 0 failed。日志：`/tmp/pb06/logs/newgreen-b2.log`（首次 B2 为 cargo CLI 语法错误：多 filter 须置于 `--` 之后；仅修命令，无代码改动）
- 跨进程观察（`--nocapture` 重跑取证）：`cross-slide premise observed: probe at 0x102950ea8 in the capture process vs 0x100514ea8 in the second spawned process`——两个自 spawn PIE 进程同函数地址不同（ASLR slide 实际观察到）；capture/recovery 全帧 raw IP 逐字保真、零归因。日志：`/tmp/pb06/logs/crossslide.log`

## 最终全 crate suite（一次，收敛后 lock，locked+offline）
- `cargo test --locked --offline -p crash-handler` → exit 0：lib 31 passed/0 failed；integration 9 passed/0 failed/1 ignored（subprocess entry）；ownership 3 passed/0 failed/1 ignored；doctest 1 passed。PB-05 ownership 与恢复持久化 suite 全绿未动。
- `cargo fmt --check -p crash-handler` → exit 0。
- 日志：`/tmp/pb06/logs/fullsuite.log`

## 限制
- 真实 Windows 未运行（无 Windows 主机）：raw 保真是格式层行为，与平台无关，但 Windows 恢复路径本变更未现场验证。
- 未新增 module identity/load mapping：离线符号化仍需崩溃时 binary 及其 load mapping，format v1 未记录，报告已如实说明；本变更不声称实现自动跨启动符号化。
- 全程未使用 pid/version 相等作为放行授权；未关闭 ASLR、未构造相同 slide 假象。
- 未观察到范围外的其他问题。
