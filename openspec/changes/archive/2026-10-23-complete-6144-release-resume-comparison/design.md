# 方法

沿用[此前对照方法](https://github.com/LordCasser/grow/blob/main/openspec/changes/archive/2026-10-23-compare-release-resume-v2-4-3-v2-5-0/design.md)的6144轮配置：每轮32 KiB assistant，50×120 PTY、诊断off、温热页缓存，独立HOME/GROW_HOME和进程，原256 MiB夹具预算。两个版本仍用已核验SHA的官方macOS ARM64 release-dist包。

复用未修改的release_resume_comparison测试。pair0排除预热且完整wire正文/角色/顺序校验；pair1..5按BA/AB交替，每对独立observer。计时仍为首历史完整帧、末条marker完整帧、Loading消失后未提交草稿回显。推理请求0，Ctrl+D退出0。正式每对0.5秒编译监测，污染整对重测，不按耗时选择；首对前10秒安静窗口，最多20次/对。自有编译完成后才计时。

仅发布五对有效样本中位数和min/max；不与前轮部分6144样本拼接，不把首屏代替完整ready，不分离就绪/分页。保留混合媒体/冷盘/硬件适用性限制。最后只清理本轮专用target和临时下载/日志/隔离目录。
