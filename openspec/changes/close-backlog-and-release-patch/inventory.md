## 清理范围

本次按用户“完成所有修复”处理缺陷、测试债务和待证实边界。此前明确暂缓的新功能与依赖真实负载的优化，不按缺陷批量实施；用户已回复“清完缺陷与测试债务，暂缓项保留说明”。无依据候选给出证据结论，不以删除历史冒充修复。

| 事项 | 实施/核对记录 | 验收状态 |
| --- | --- | --- |
| ACP worker watcher 阻塞初始化 | unblock-acp-during-watcher-registration，真实 Pager worker 及 leader/stdio；OS调用不可取消 | 最终 Shell/Pager 全量通过，启动gap/config独立于skills已测 |
| LSP mock refresh 时序 | synchronize-lsp-refresh-test，response9002后握手 | 完整 Tools 2428通过 |
| SideQuestion receiver 生命周期 | cancel-abandoned-side-questions | 最终 Shell 全量通过 |
| native 分段 AgentMessage 交错 | audit-runtime-native-message-tool-batch-boundary | 生产整批drain不可达；保留adapter/未来调用限制 |
| 不可验证 child 完成恢复 | require-canonical-unavailable-child-result | 真实文件red、子会话168、最终 Shell通过 |
| debug-only panic 测试 / Chrome计时 | clarify-committed-anchor-release-test、emit-chrome-instrumentation-timers | debug通过，release cfg单独验证；diagnostics54通过 |
| Pager sticky几何 / Markdown clone | fix-pager-sticky-navigation-geometry及audit | Pager7620通过；clone未进入生产热点 |
| 后代展示跨实体 / 无ID重入 | fix-descendant-presentation-replay | 标准Seed/Result校验、真实typed metadata，Shell/Pager全量通过 |
| tmux清理预算判别 | test-tmux-post-exit-cleanup-budget | handler/marker握手、expired deadline，pager-render1022通过 |
| Git构建身份（原两条合并） | fix-build-git-identity-invalidation | real Git normal/worktree/detached/packed/nested-packed 4通过 |
| PTY独立Leader所有权 | own-pty-leader-test-daemons | test-support71、真实多客户端PTY5、真实daemon Drop/panic回收1通过 |
| OCR / 并发fixture清理 | own-local-ocr-process-tree | 取消/超时/正常返回带后代均通过最终Shell |
| Plan handoff消费核对 | fix-plan-handoff-consumption-recheck | 同Turn幂等、其他Turn/Dismissed拒绝；Plan130通过 |
| Goal各Control生命周期scope | fix-goal-control-context-scope | pause真实red、Goal194、ChatState602、最终Shell通过 |
| 正式门禁覆盖新增runtime回归 | gate-backlog-runtime-regressions | 追加4包及真实daemon测试；Python34通过（1skip）；正式CI由发布记录补齐 |
| 发布门禁新检出的日志测试时序 | synchronize-unified-log-overflow-test | diagnostics55及200次精确执行通过 |
| 发布PTY沿用旧活动组契约 | align-activity-group-pty-contract | 最终交互+3次重复、余下16项发布PTY全通过 |

历史原表完整保存于 [backlog-before](backlog-before.md)。最终命令、源码范围、正式发布与产物由 verification.md 记录。上述通过均为本地记录，发布门禁单独列证据。
