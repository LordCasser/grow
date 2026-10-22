# 验证与审查

已同步 [公开 2.5.0 release](https://github.com/LordCasser/grow/releases/tag/v2.5.0) 正文与仓库 notes；API 读回全文一致。此次是发布后的说明更新，tag 内原 notes 不修改，原发布记录的一致性结论限定为“发布时”。

核对三份已归档 release 配对记录，并由独立只读数值复核确认：2048 轮 C3 RPC 747.114→416.701 ms，下降 44.22%；P1 默认诊断关闭末屏 1384.2→560.9 ms，下降 59.48%；扫描优化末屏 552.8→428.6 ms，下降 22.47%。表内秒数和整数百分比为四舍五入。三条链接均固定到最终发布提交 8443a4d1648d17b10f993c845cb68d949c55235e，git cat-file 逐一确认文件存在。

对抗性复核：未使用 debug 的 6.69 秒，未把末屏当作就绪，未把三个独立阶段合并成 2.4.3→2.5.0 的统一对照；明确 C3 的 Rust 1.93.1 与 TUI 的 Rust 1.99.1、普通 release 与 release-dist 差异、温热缓存、n=3 中位数以及短会话/首屏没有一致提升。未声称所有消息类型获得同幅收益。

编辑前后读取 GitHub API，比对 release id 420909068、标题、tag、target_commitish、draft/prerelease、published_at、latest id，均未变；annotated tag 对象 f1e99dff7af3b499f8c8d432ba7e9d745b7c8829 未变。11 项资产 id/name/size/digest/download URL 均相同，未重新上传或重建。只改 release 正文。

Markdown 链接全部为绝对地址，git diff --check 通过。纯文档更新未运行 Cargo 构建或性能复测，无新增编译缓存。

归档后：Totals: 16 passed, 0 failed (16 items)；Totals: 702 passed, 0 failed (702 items)。临时 API 快照已清理。
