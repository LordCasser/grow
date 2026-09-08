## Regression
旧实现真实 dispatcher 回归失败于 copy must use session cwd。旧进程相对位置由 tempdir_in 管理，结束自动清理，不留下错误目标文件。

## Results
修复后 app::root::dispatch::tests::transcript：13 passed，0 failed。新回归验证会话目录中的真实内容、进程目录未写入、绝对目标不变，以及 Unix 两目标权限均为 0600。既有 export 和查看器测试继续通过。

locked/offline、无增量、debug=0、2 jobs、RUST_MIN_STACK=16777216；仅有既有 macOS compact unwind 警告。

## Limits
未操作真实剪贴板、默认备份、HOME 或进程 cwd。~ 使用原 shellexpand 规则，未额外测试 HOME 展开。复制文件原子提交单独登记 backlog，未声称本轮修复。CLI 未重新链接。
