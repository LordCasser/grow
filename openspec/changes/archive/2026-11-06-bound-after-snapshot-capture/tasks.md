## 1. Contract

- [x] 1.1 阅读 OpenSpec 索引、client-surfaces rewind 相关 spec、backlog「Workspace after-snapshot阻塞prompt返回」，核对 `end_prompt` 调用方与 shell 预览消费端。
- [x] 1.2 建立 change（proposal、delta WHEN/THEN、design、tasks）并通过 `openspec validate --all --strict --no-interactive`。

## 2. Implementation and verification

- [x] 2.1 workspace：`end_prompt` 固定 5s 总预算捕获（共享绝对 deadline + 8 路）、三态结果映射、重试先清旧该轮 after、`merge_rewind_points_from` 删除未知轮前轮同 path 旧 after。
- [x] 2.2 shell：预览判定改为最新触及 point + 其 after 三态，`snapshot_unavailable` 不进 clean、preview unknown 提示、`force=true` 语义不变。
- [x] 2.3 测试：受控 FS 五路矩阵、公共路径固定预算 watchdog、重试清旧、merge、serde/lazy 往返全部运行通过；Shell 实际 handle_rewind 的 unknown/stale/force 三项通过。原实现的公共路径 watchdog 未单独实跑，保留此证据限制，不声称已有 old-red。
- [x] 2.4 仅 rustfmt 本次触碰的 Rust 文件，父核完整 diff 与默认测试产物，Workspace file_state 41/41、Shell rewind 3/3；开发说明与验证记录同步。
