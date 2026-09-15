# 验证记录

## 故障证据与复现

- 用户会话：`01a0a319-0e5c-7252-86c4-0efcee4671da`。Timeline 的 2461/2464 为自动压缩开始/失败；2471/2474 为手动开始/失败，两次选区失败各约 8 ms，无摘要 provider 调用。
- 修复前只读重放：260 个 Surface 项及 260 个身份；prompt 位置 7、180。40,960 和 32,000 保留预算返回 `None`；16,000 才能选择 7..179。确认不是 Goal 投影造成数量错位。
- 新增 `partial_compaction_splits_older_segment_when_recent_prompt_tail_is_short` 在旧实现失败；来源阈值及保留预算均不变即可修复。
- 新增 `compact_extension_preserves_actor_error_outcomes` 经真实 `ext_method` → 扩展处理器 → SessionCommand responder，在旧实现失败：结构化 `grow/controlTerminalPublished` 被改成字符串。

## 修复后验证

| 入口 | 结果 |
| --- | --- |
| `CARGO_BUILD_JOBS=4 RUST_MIN_STACK=16777216 cargo test --locked -p chat-state --lib` | 485 passed，1 个既有 ignored，0 failed |
| `CARGO_BUILD_JOBS=4 RUST_MIN_STACK=16777216 cargo test --locked -p shell --lib compact -- --test-threads=4` | 133 passed，0 failed；包含手动 RPC、异步边界、取消、回退及上下文溢出回归 |
| `CARGO_BUILD_JOBS=4 RUST_MIN_STACK=16777216 cargo test --locked -p pager --lib compact -- --test-threads=4` | 80 passed，0 failed；已发布失败不显示 unknown，无标记传输失败仍显示 unknown |
| `rustfmt --edition 2024 --check`（修改的生产 Rust 文件及纯函数测试文件） | 通过；未批量格式化其他测试文件的既有格式差异 |
| `git diff --check` | 通过 |
| `openspec validate --all --strict --no-interactive` | 19 passed，0 failed（归档前） |

实际会话用 [surface_probe.rs](surface_probe.rs) 只读重放到两次 Compaction Started 前，使用生产 Timeline/Goal projection/选区函数。探针临时作为 chat-state example 构建，验证后已从 crate 移除。完整数值见 [探针输出](surface-probe-result.txt)。

- 自动压缩：选择 8..136，来源估算 114,940 tokens，保留 tail 42,249 tokens ≥ 40,960。
- 手动压缩：同一区间，保留 tail 42,314 tokens ≥ 40,960。
- 被选区与 tail 的 tool call ID 集合均与结果集合相等；保留最早 prompt。原会话和 Goal 状态未修改，未发送真实模型请求。

## 范围与限制

本修复已验证本地选区、模拟 provider 压缩路径和客户端反馈，不保证真实外部 provider 的摘要成功。未提交 Git、未替换正在运行的 Grow；运行中进程需加载新构建后才包含修复。macOS 链接已有 `__eh_frame` 大于 16 MB 的测试二进制警告，测试正常执行。

## 归档与清理

已完成场景核对并归档；归档后的 `openspec validate --all --strict --no-interactive` 为 17 passed、0 failed。全部 Rust 验证结束后执行 `cargo clean`，移除 81,899 个文件、23.3 GiB 构建产物，未清理其他项目。

归档完整性校验 `openspec validate --archived --no-interactive`：336 passed，0 failed；清理后磁盘可用空间约 42 GiB。
