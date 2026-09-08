## Decision
提取小型纯反馈函数，接收 CopyResult 与导出文本，返回 Result<String>。Failed 返回错误；Confirmed/Unverified 使用 result.message 保留具体目标及确认程度，再追加共享 clipboard_stats_suffix。生产分支只在该函数成功后输出反馈；诊断记录实际 delivery，不再无条件声称复制成功。

## Verification
纯测试构造失败、确认成功和未确认结果，验证失败返回 Err，未确认消息不替换为成功，含 Unicode 的文本沿用共享统计函数（当前仍按字节计数，见独立债务）。不操作真实剪贴板或用户会话。

## Separate debt
文件导出仍使用同步直接写入；会话 cwd 与进程 cwd 的相对路径语义需单独核对，不混入本修复。
