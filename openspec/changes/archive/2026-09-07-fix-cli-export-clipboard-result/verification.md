## Evidence
旧 CLI clipboard 分支 let _ = copy_text，随后无条件输出 copied 并返回 Ok；现按 CopyResult.delivery 判断失败，再使用后端 message。CLI main 的 Export 路由直接返回 export_cmd::run 的 Result。

## Tests
export_cmd::tests：1 passed，0 failed。表驱动覆盖 Failed、Unverified、Confirmed 三类结果，确认失败 Err、成功/未确认消息原样保留并追加共享统计。

命令：cargo test --locked --offline -p pager --lib export_cmd::tests --quiet，禁用增量、debug=0、2 jobs、RUST_MIN_STACK=16777216。仅有既有 macOS compact unwind 链接警告。

## Limits
未操作真实剪贴板或用户会话。共享统计当前仍按字节计算 chars，已独立登记，不声称修复 Unicode 计数。未改变文件/stdout 分支、剪贴板路由或新增回退文件。CLI 尚未重新链接。
