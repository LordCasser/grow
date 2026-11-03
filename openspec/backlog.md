# Backlog

这里只保留已有明确暂缓决定或启动门槛的事项。v2.6.0 非延期验证债务已按本机证据闭合或明确保留残余边界；逐项 owner、结果和未完成门禁见[发布阻塞闭合矩阵](changes/close-v2-6-0-release-blockers/verification.md#八项闭合矩阵)，原始八项快照见[backlog-before.md](changes/close-v2-6-0-release-blockers/backlog-before.md)。既有历史整理记录见[本轮闭合记录](changes/archive/2026-11-03-close-nondeferred-backlog-and-build-release/algorithm-inventory.md)及其[清理前快照](changes/archive/2026-11-03-close-nondeferred-backlog-and-build-release/backlog-before.md)。

## 明确暂缓

- **普通长 prompt 导出文件寿命（待独立链路复现）**：`run_turn_via_sampler` 局部的 PromptBlobExport 在采样返回时删除 TempDir，而模型的 read_file 发生在后续工具阶段；需独立复现普通 prompt 的采样→全文读取，再确定既有 artifact owner 的寿命边界。当前现场走 steer，未使用该导出路径，不并入插话保真修复。证据和范围见[长输入调查](changes/archive/2026-11-03-preserve-long-steering-input/design.md#separate-audit-finding)。

- **共享布局以外的字素边界（待对应复现）**：本次 [字素布局修复](changes/archive/2026-11-03-fix-tui-grapheme-layout/design.md) 限于普通文本共享内置换行、截断和搜索列映射。`ratatui-inline/src/segment.rs` 的 ANSI 历史行拆分仍按 scalar 计宽，`pager-render/src/render/terminal_output.rs::TermSink::put` 的 ANSI cursor model 仍按字符推进；跨样式 span 的字素归属也需先与 Ratatui 的逐 span 绘制语义统一。若出现对应 Minimal resize、带 cursor movement 的中文工具输出或跨 span 组合符号复现，再分别建立 change 验证各自所有者，不能将本次字体回退截图当成这些路径的复现。

- **跨路由 reasoning 后续边界**：[第一阶段审查](changes/archive/2026-10-29-improve-cross-route-reasoning-compatibility/audit.md) 发现通用 native-bearing HTTP 400 仍会触发一次 reset，以及可见 reasoning 仅保存 text、未区分摘要和完整正文。本阶段修复错误事实保真和目标请求投影；若真实目标要求更强语义，应独立处理精确 continuation 拒绝分类或最小 provenance，不预建全局 capability registry。

- **Remote Control / Agent Host 跨设备使用（暂缓、未排期）**：保留方向，不实施、不设日期；未来启动时另建 change，明确身份、权限、执行 owner、断线和恢复边界。不得据此引入 Dock、Memory 或远端控制框架。决策见 [上游主题评审](changes/archive/2026-10-18-record-upstream-review-2bdd1d6/review.md)。

- **Provider 缓存策略与 native epoch（待真实路由证据）**：[理论 review](changes/archive/2026-09-28-evaluate-provider-cache-affinity/results.md) 仅证明本地 key/breakpoint/retention 机制。先取得具体 provider/model/deployment、字段透传和真实负载证据，再做该 route 的最小优化，不预建 registry。

- **Recap 与主请求缓存（待真实共享前缀及收益证据）**：recap 保持独立 key、tool-free Sideband 契约；不为假设中的缓存复用添加主请求 tools。先测真实收益。

- **跨多个合法选区连续摘要（契约选择，未启动）**：当前一次有界摘要后仍超窗会保留已提交事实并终止回合；继续压缩更多选区需另立契约，不增加无界重试。

- **MCP 多轮 elicitation（现有独立提案）**：宿主 SDK 接点仍未通过既定门槛，保留 [提案与任务](changes/add-mcp-multi-round-elicitation/tasks.md)，不把未实现功能列为本轮缺陷。

- **ProcessScope supervisor 与 leader reap 的身份同步（待受控 PID/PGID 复用证据）**：terminal 已在最终组信号后、reap 前撤销自身强引用；全局 `kill_all` 若更早 upgrade Weak，仍有独立的跨线程收尾交错。先在可控制 PID 复用的隔离环境验证，不凭假设引入全局 owner/锁协议。范围和当前证明边界见[命令组审查](changes/archive/2026-11-04-finish-command-groups-before-leader-reap/review.md)。
