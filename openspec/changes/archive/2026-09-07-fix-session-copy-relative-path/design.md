## Decision
在调用 write_text_to_copy_file 前完成会话相对路径解析，绝对目标保持原样。helper 对默认备份的调用继续按原来的路径规则工作。

## Separate debt
复制文件的 write_owner_only 先 truncate 再 chmod/write，仍存在失败损坏旧文件问题，需在保证强制私有权限和默认备份语义的前提下独立修复，不能直接复用保留旧权限的 export helper。

## Verification
真实 dispatcher 回归用受控临时目录捕获进程相对位置，检查会话路径文件内容、绝对目标与 Unix 私有权限；不触发真实剪贴板。
