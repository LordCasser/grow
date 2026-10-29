## 验证环境

2026-10-29，macOS arm64，Rust 1.93.1。工作区原有 dirty 更改保留；本轮没有 commit、push 或发布。Cargo 使用系统 PATH、CARGO_INCREMENTAL=0、CARGO_PROFILE_DEV_DEBUG=0、CARGO_BUILD_JOBS=4、RUST_MIN_STACK=16777216。默认 CLI 检查未关闭默认 features。

## Red → 修复 → Green

- 新增实际 SessionActor/provider 反例：79K/100K +25K 预留，在修复前因104011总需求直接失败；40K预留的保留过大案例没有任何 Summary（期望1）。证据 `/tmp/grow-composed-output-red3.log`。这两项隔离了缺失的预压缩入口。
- Goal route/clear 两个顺序在请求 build 前均得到旧scope压力99，当前授权预期101；修复前7通过、2失败。证据 `/tmp/grow-composed-goal-red.log`。
- 修复后输出成功路径恰好一份 durable summary +一次普通请求；总需求仍超限时恰好一份 summary、零普通请求且 Completed 保留。
- 74K/100K +27K 输出、低于75%异步预阈值的专用夹具：仅走一次模型无关 prune，再一次普通请求，零 summary，一次 Completed。
- 图片真实 ImageProjection 后输入低于普通阈值，但总需求仍超窗，自动路径继续进入 compaction transaction；不能把图片减少误判为总预算已满足。
- 后台 promotion 复用原provider attempt，测试取消后再次等待、取消generation拒绝提交以及40K reserve仍超限；Timeline Completed、UI Completed均核对，pending notice/owner已清理。
- 低值锚点经历两次压缩、再增长、重复 build 及提交前 inactive transaction 拒绝；滚动参考、计数、Surface identity 与累计消费逐步核对。
- Goal validated Timeline restore 后实际 route admission 与 public rewind 拒绝双顺序；Active/Paused/Complete guard；clear后两个可达顺序均成功 rewind，旧历史标签不能复活 Goal。请求 build 前就核对 scope 压力。

## 最终回归

| 命令/范围 | 结果 | 日志 |
| --- | --- | --- |
| `cargo test --locked -p chat-state --lib` | 600通过，0失败，3 ignored | `/tmp/grow-composed-chat-final.log` |
| Shell lib `compaction` 全过滤集 | 125通过，0失败 | `/tmp/grow-composed-shell-final.log` |
| Shell lib `goal` 全过滤集 | 193通过，0失败 | `/tmp/grow-composed-goals-final2.log` |
| `cargo check --locked -p cli` 默认features | 通过 | `/tmp/grow-composed-cli-check.log` |
| `git diff --check` | 通过 | 当前工作区 |
| `openspec validate --all --strict --no-interactive` 归档前 | 17通过，0失败 | `/tmp/grow-composed-openspec-before.log` |

压缩集包含已有真实持久化文件+SIGKILL恢复用例、ACK fault、route通知、取消/重提升等周边回归。新增低值和 Goal 组合使用真实actor +受控persistence；不冒称完成了真实provider/完整CLI冷重启/整机断电。完整范围和夹具修正见 [analysis](analysis.md)。

永久 persistence 失败按设计关闭 ChatState owner，因此失败 Goal clear 不能继续从原 actor 读取状态。回归改为验证运行时原授权保留、owner关闭，并从最后 acknowledged Timeline 与原授权冷恢复核对scope/压力；这属于夹具纠正，不改生产持久化策略。

## 收尾

已归档为 `2026-10-29-verify-composed-context-transitions` 并合入两项 model-sampling Requirement。

- 归档后 `openspec validate --all --strict --no-interactive`：16通过，0失败。
- `openspec validate --archived --strict --no-interactive`：723通过，0失败。
- `cargo clean`：删除52,721个构建文件，释放9.1GiB；相关测试和默认CLI检查均已完成后执行。
- `git diff --check`：通过。无 commit/push/发布。
