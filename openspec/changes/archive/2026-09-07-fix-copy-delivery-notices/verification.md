## Evidence
旧 /copy 与 /export 的 Clipboard 分支忽略 result，固定使用 Copied to clipboard 文案。Clipboard 分支可包含 Unverified；toast_message 先前已经使用后端反馈。本轮持久摘要使用相同后端语义并额外保留确认成功的备份路径。

## Results
- pager-render copy_summary_preserves_delivery_evidence_and_backup：1 passed，0 failed。矩阵为 native/tmux/unverified × 有无备份，加 File/Failed 共 8 个组合；未确认摘要明确断言 Copy sent 且不含 Copied。
- Pager transcript dispatcher：14 passed，0 failed。生产两个调用点编译成功，既有文件/选择/导出回归继续通过。
- locked/offline、无增量、debug=0、2 jobs、RUST_MIN_STACK=16777216；Pager 仅有既有 macOS compact unwind 警告。

## Limits
没有触发真实剪贴板；dispatcher 回归不模拟远程 OSC 52，远程反馈由纯枚举矩阵和调用点核对支持。底层路由、备份、toast 时长不变。CLI 未重新链接。
