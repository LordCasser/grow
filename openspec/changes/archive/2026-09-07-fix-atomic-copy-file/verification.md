## Results
- pager-render copy_file_：4 passed，0 failed，覆盖原父目录创建/内容、0600 收紧，以及新增部分失败保留和 symlink 场景。
- pager copy_file_uses_session_cwd_and_keeps_private_permissions：1 passed，0 failed，真实 dispatcher 的相对/绝对文件及权限继续成立。
- locked/offline、无增量、debug=0、2 jobs、RUST_MIN_STACK=16777216。Pager 仅有既有 macOS compact unwind 警告。

## Fault evidence
写入闭包内确认临时文件已为 0600，写入 partial 后返回错误；原文件 old content 和原 0644 权限保留，目录无临时文件。成功后原私有权限回归确认收紧为 0600。目录、悬空链接、只读目标拒绝，链接更新保留节点。

## Limits
故障由闭包注入，不模拟真实磁盘耗尽或掉电。未设置 GROW_COPY_FILE/HOME，未写用户默认备份；默认备份到同一 helper 的调用与私有目录创建由源码核对。没有执行真实剪贴板。非 Unix 平台未运行，目录 fsync 与抗恶意路径竞争不在本变更保证中。CLI 未重新链接。
