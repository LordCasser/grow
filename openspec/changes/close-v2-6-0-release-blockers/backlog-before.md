# Backlog

这里只保留已有明确暂缓决定或启动门槛的事项。已完成缺陷、测试债务和已评估关闭的优化候选移出活动清单；逐项证据见[本轮闭合记录](changes/archive/2026-11-03-close-nondeferred-backlog-and-build-release/algorithm-inventory.md)和[清理前原始快照](changes/archive/2026-11-03-close-nondeferred-backlog-and-build-release/backlog-before.md)。历史记录保留在各自归档中。

## 明确暂缓

- **普通长 prompt 导出文件寿命（待独立链路复现）**：`run_turn_via_sampler` 局部的 PromptBlobExport 在采样返回时删除 TempDir，而模型的 read_file 发生在后续工具阶段；需独立复现普通 prompt 的采样→全文读取，再确定既有 artifact owner 的寿命边界。当前现场走 steer，未使用该导出路径，不并入插话保真修复。证据和范围见[长输入调查](changes/archive/2026-11-03-preserve-long-steering-input/design.md#separate-audit-finding)。

- **共享布局以外的字素边界（待对应复现）**：本次 [字素布局修复](changes/archive/2026-11-03-fix-tui-grapheme-layout/design.md) 限于普通文本共享内置换行、截断和搜索列映射。`ratatui-inline/src/segment.rs` 的 ANSI 历史行拆分仍按 scalar 计宽，`pager-render/src/render/terminal_output.rs::TermSink::put` 的 ANSI cursor model 仍按字符推进；跨样式 span 的字素归属也需先与 Ratatui 的逐 span 绘制语义统一。若出现对应 Minimal resize、带 cursor movement 的中文工具输出或跨 span 组合符号复现，再分别建立 change 验证各自所有者，不能将本次字体回退截图当成这些路径的复现。

- **跨路由 reasoning 后续边界**：[第一阶段审查](changes/archive/2026-10-29-improve-cross-route-reasoning-compatibility/audit.md) 发现通用 native-bearing HTTP 400 仍会触发一次 reset，以及可见 reasoning 仅保存 text、未区分摘要和完整正文。本阶段修复错误事实保真和目标请求投影；若真实目标要求更强语义，应独立处理精确 continuation 拒绝分类或最小 provenance，不预建全局 capability registry。

- **Remote Control / Agent Host 跨设备使用（暂缓、未排期）**：保留方向，不实施、不设日期；未来启动时另建 change，明确身份、权限、执行 owner、断线和恢复边界。不得据此引入 Dock、Memory 或远端控制框架。决策见 [上游主题评审](changes/archive/2026-10-18-record-upstream-review-2bdd1d6/review.md)。

- **Provider 缓存策略与 native epoch（待真实路由证据）**：[理论 review](changes/archive/2026-09-28-evaluate-provider-cache-affinity/results.md) 仅证明本地 key/breakpoint/retention 机制。先取得具体 provider/model/deployment、字段透传和真实负载证据，再做该 route 的最小优化，不预建 registry。

- **Recap 与主请求缓存（待真实共享前缀及收益证据）**：recap 保持独立 key、tool-free Sideband 契约；不为假设中的缓存复用添加主请求 tools。先测真实收益。

- **跨多个合法选区连续摘要（契约选择，未启动）**：当前一次有界摘要后仍超窗会保留已提交事实并终止回合；继续压缩更多选区需另立契约，不增加无界重试。

- **MCP 多轮 elicitation（现有独立提案）**：宿主 SDK 接点仍未通过既定门槛，保留 [提案与任务](changes/add-mcp-multi-round-elicitation/tasks.md)，不把未实现功能列为本轮缺陷。

- **ProcessScope supervisor 与 leader reap 的身份同步（待受控 PID/PGID 复用证据）**：terminal 已在最终组信号后、reap 前撤销自身强引用；全局 `kill_all` 若更早 upgrade Weak，仍有独立的跨线程收尾交错。先在可控制 PID 复用的隔离环境验证，不凭假设引入全局 owner/锁协议。范围和当前证明边界见[命令组审查](changes/finish-command-groups-before-leader-reap/review.md)。

## 独立验证债务

- **v2.6.0 候选：macOS 配置监听与 MCP 门禁失败（发布阻塞）**：本机固定 Rust 1.93.1 的 Pager 全组有 11 项 MCP 热更新/cold-load 失败，Shell 有三项目录监听事件超时；串行 watcher 组仍复现 late-directory/recreation 事件缺失。native sample 证明动态 `ConfigFileWatcher::apply_session_update` 在 ACP LocalSet 同步进入 FSEvents `watch_inner/run` 等待。独立 MCP 用例可通过，尚不能把全部失败归因于同一点。需另建 change 处理动态监听所有权与取消/释放边界，并重新验证公开 MCP 更新/冷加载和目录重建；不能用延长 fixture 超时、skip 或只跑一次通过替代门禁。[候选审查与证据](changes/archive/2026-11-03-prepare-v2-6-0-release/review.md#未解除的发布阻塞)。

- **v2.6.0 候选：原树出生权限 cold-load 回归失败（发布阻塞）**：`suspended_child_cold_restore_retains_birth_authority_end_to_end` 在全组及独立运行中均于 `authority.cold_load` 的十秒 fixture 期限失败。先定位恢复等待 owner 和原始出生事实链，再核对 MCP narrow/removed、缺事实拒绝与逐次授权；当前不能宣称这是权限放大，也不能宣称恢复验证已通过。调查与修复单独立项，不改已有出生授权限制来使测试通过。[候选审查](changes/archive/2026-11-03-prepare-v2-6-0-release/review.md#未解除的发布阻塞)。

- **子任务观察的跨层故障验证（已修复后的覆盖边界）**：已确认的入口、期限、终态、中断、采样取消和active发现问题由[修复记录](changes/archive/2026-11-03-fix-subagent-observation-blindspots/verification.md)结清。仍需独立验证真实SessionActor中terminal/干预/receipt ACK交错、ChatState回复停答与过期查询积压、结果worker饱和、进程异常及跨平台；当前没有证据把这些全部认定为已发生故障。活跃注册真正丢失与主模型忘记ID须分别取证，不能把无路由或空active清单当清理凭据。原审计的未验证项目见[边界清单](changes/archive/2026-11-03-audit-subagent-observation-blindspots/analysis.md#后续验证缺口)。


- **外部变异测试的资源边界与中断恢复（待独立设计）**：已在 Grow 会话启动的 Solaris 测试中采到 97.0G / 104.8G footprint：关闭环检测后无限追加字符串，约五分钟的工具超时虽结束进程树，仍来不及避免系统换出；中断还跳过脚本末尾的恢复并污染下一轮变异。需要分别确定高风险测试的资源预算、临时副本清理和中断结果解释的责任边界，不能把超时直接当 mutation 验证通过，也不能混入已完成的前端 transcript / retiring 回收修复。证据见[现场调查](changes/archive/2026-11-03-analyze-runaway-mutation-test-memory/analysis.md)。

- **辅助指定模型的实际路由（待跨 provider 验证）**：AI suggest、prompt suggest、memory note rewrite 的指定模型仍可能只覆盖 request.model，client 沿主路由；其跨 provider 解析和配置读取风险应分别验证。Memory Flush 已独立完成[完整 transport 解析及真实跨 provider 验证](changes/archive/2026-11-03-resolve-memory-flush-model-route/verification.md)，不再列为此项未修复范围。先以不同 endpoint/backend/认证域 fixture 验证，并分别明确 catalog alias 与 provider model 字符串语义；复用现有辅助 resolver，不扩建能力 registry。边界见 [A3/A4 审计](changes/archive/2026-11-03-analyze-auxiliary-image-request-paths/analysis.md)。

- **旧 PTY Responses 工具流夹具**：`pager/tests/pty_e2e/common.rs` 的单工具和 parallel Responses helper 缺少 function item 宣告，真实 Responses parser 会拒绝其 arguments delta；Chat twin 成功不能证明这两个分支有效。证据见 [退出/恢复验证](changes/archive/2026-11-03-fix-background-child-exit-and-resume/verification.md)。本次场景复用完整的 test-support 流，不放宽生产校验；另开测试维护 change 统一这些旧 helper 并运行其 Responses 调用方，不混入退出生命周期修复。

- **临时工作目录的 flush 回执（本轮真实进程验证发现，独立处理）**：MemoryStorage 对 OS 临时目录按既有 ephemeral 策略跳过 `write_daily_log`，但返回预期 Path，调用方仍可能发 `written`。应另立存储结果契约明确 skipped 与 written，不取消临时目录禁写策略，也不能凭返回 Path 声称文件存在。本轮持久化 E2E 将隔离 cwd 放在 ignored target 下，并逐一读取实际 Markdown，避免用临时目录的成功回执代替落盘证明。

- **取消入口的command/subagent职责（独立处理）**：输出与观察工具已[拆分](changes/archive/2026-11-03-split-subagent-observation-tools/verification.md)，但`KillTerminalCommandTool`仍委托混合`KillTaskTool`，先访问Terminal再尝试子任务backend；两个工具共享KillTaskAction。当前默认混合取消保留，尚无独立的无Terminal子任务取消契约。若启动取消拆分或需要非Terminal宿主取消child，另立change核对对象隔离、来源权限、幂等与真实取消收据，不只改名称。代码证据与边界见[审计](changes/archive/2026-11-03-split-subagent-observation-tools/audit.md)。
