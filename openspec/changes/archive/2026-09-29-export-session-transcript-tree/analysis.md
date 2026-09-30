# 代码分析与交接基线

2026-09-29；本轮只读产品代码，新增 OpenSpec 文档。Atlas 用于 scoped symbol 导航，部分返回路径/行号落后于工作树，以下结论以实际读取的文件和符号为准。没有执行 Cargo 构建或产品测试。

## 1. 已确认的产品边界

- trajectory 可以导出与用户会话展示一致的对话记录。
- CLI export 默认创建 `./<session-id>/`，递归保存主 agent 和 subagent 的 Markdown transcript。
- 用户明确选择 CLI 所有输出都采用目录，删除 `--clipboard`；TUI `/export` 继续导出当前视图的单份 transcript，语义暂不改变。
- replay 是 CLI 内无输入框的只读 TUI，复用现有会话界面，支持指定倍速。
- replay 需要流式输出过程，但允许近似节奏，不要求记录/恢复原始 token 到达时间。
- 用户明确要求本轮详细分析、制定方案并 OpenSpec 立项，随后切换其他 agent 实现；拆成两个依次执行的 change。

## 2. 已有入口和信息丢失位置

| 位置 / 符号 | 当前行为 | 本需求的影响 |
| --- | --- | --- |
| `crates/codegen/cli/src/main.rs`，`Command::Export` / `Trajectory` | 在 CLI 分发中直接调用 Pager 命令入口；Export 不初始化执行 session | 保留离线入口，不绕到 resume |
| `crates/codegen/pager/src/app/cli.rs`，`Command::Export` | 帮助为单 session Markdown export | 参数帮助需要同步目录行为 |
| `crates/codegen/pager/src/export_cmd.rs`，`ExportArgs` / `run` | `session_id`、可选文件、clipboard；默认 stdout；只读一个 session | 没有子树遍历；没有目录发布 |
| `crates/codegen/shell/src/session/storage/mod.rs`，`load_updates_for_replay` / `for_each_reconciled_replay_line` | 校验 Timeline、执行 reconciliation 后只返回 ACP 的 `SessionUpdate`；丢掉通知 envelope/meta，忽略 Grow | 不能直接拿来作为完整导出或定时 replay 数据源 |
| 同文件，`with_reconciled_replay_lines` / `reconcile_raw_replay_lines` | rewind 过滤、canonical response 对账与插回候选锚点 | 必须复用这些 authority/排序规则，不能裸读 updates 绕过 |
| `crates/codegen/pager/src/acp/tracker.rs`，`AcpUpdateTracker::handle_update` / `is_task_tool` | ACP 转 Scrollback；Task 被隐藏，因为 Grow Subagent 展示拥有该 UI | 只导出 ACP 会连父级 Task 展示也遗漏 |
| `crates/codegen/pager/src/scrollback/export.rs`，`render_blocks_to_markdown` | 跳过 thinking、subagent、system 等；工具为单行摘要 | 这是有意的紧凑导出，不是完整用户展示 serializer |
| `crates/codegen/pager/src/app/root/dispatch/transcript.rs`，`dispatch_export_conversation` | TUI 取当前活动 root/child 的内存 scrollback；相对路径基于 session cwd；使用同一个紧凑 renderer | 不应随 CLI 改成目录，也不能无意改变 `/transcript` |
| `crates/codegen/pager/src/scrollback/block.rs`，`RenderBlock::copy_text` / `copy_meta` | 已覆盖多种正文与工具内容，但有些 block 返回 None；BgTask 正文另在 store | 可以复用，不能仅遍历 copy_text 就宣称完整 |
| `crates/codegen/pager/src/app/acp_handler/` | Grow notice、Hook、子 agent、后台任务、Goal/Workflow 等转换分布于 handler 中 | 提取所需纯展示部分；不能离线调用会发送交互响应的总 handler |
| `crates/codegen/pager/src/app/subagent.rs`，`durable_child_spawns` / `replay_inherited_updates` | 子会话由 Timeline spawn 丰富元数据，单独重放 child transcript；当前一些读取失败是 best effort | 完整导出必须将缺失/损坏变成明确失败，不能复用静默忽略策略 |

## 3. 三种关系不能混用

1. `Timeline::branch_transcript` 是未压缩的当前分支对话；模型 Surface 会受压缩、图片投影和修复影响，不等于用户历史。见 `chat-state/src/timeline.rs`。
2. `SubagentSpawnEvent.child_session_id` 指向独立 child ledger；`SubagentSeedEvent.parent_timeline_id + parent_spawn_seq` 证明 lifecycle owner。`Timeline::validate_subagent_seed_link` / `validate_subagent_result_link` 已有严格校验。
3. `SubagentSpawnEvent.security_parent_session_id` 是直接委派者。`shell/src/agent/subagent/mod.rs` 的 `SubagentCtx` 明确说明嵌套 Task lifecycle 会被 coordinator 重挂到 root；`handle_request.rs` 同时落下这两套身份。`Summary.parent_session_id` 也用于 fork，不能独立拿它建真实委派目录树。

因此「扫描 parent summary 并递归」和「照 trajectory 的 lifecycle tree 排目录」都不充分。要先校验 spawn/seed 身份，再用直接委派边安排输出层级。请求根本身是 child 时，需要借其 seed 找到 lifecycle owner 中的相关 spawn 索引，然后只选择该 child 的直接委派后代；不能顺带导出同级 agent、祖先对话或 `resumed_from` 来源。

目录树描述已持久的委派关系，每个节点正文仍遵循该 session 自己的 rewind 分支。父级 rewind 不删除已经发生的 spawn 事实；不能因一条 Task UI 行暂时不在正文就擅自删除其已记录 child。父 transcript 的独立子文档索引用来表达这种关系，不伪造历史消息。

## 4. 可以复用的存储与安全边界

- `storage/jsonl/mod.rs`：`open_session_by_id_shared_read` 返回身份核对过的目录能力，不进入 writer cache；适合独立 reader 与 Windows 活跃 writer 共存。
- `OpenedSession::timeline_events` / `validated_timeline` 及目录能力提供固定来源读取。需要的 prompt blob、sideband 或 artifact 沿现有完整性边界解引用，不按正文路径猜测读取。
- `session/trajectory.rs::refresh_tree_from_directory` 已示范 parent summary、spawn/seed、terminal result link 校验；它是调试器的跨 ledger 遍历，不能直接作为用户 transcript renderer。
- `session-timeline` 的只读观察不修复 Summary；`client-surfaces` 的 read-only response reconciliation 只在内存补投影；Hook 历史 publication 是 observational。
- `export_cmd.rs::write_export_file` 已有单文件临时写入、sync、原子替换，不能由此推断整目录原子性。
- `storage::ContainedDirectory::publish_child_no_replace` 与 `pager/src/local_drafts.rs::rename_no_replace` 有不覆盖发布的先例。实现应复用或窄化抽取适用原语，不用“exists 检查后普通 rename”声称无竞争覆盖。

## 5. trajectory 与 Pager 的依赖方向

`pager` 依赖 `shell`；trajectory HTTP server 和 HTML 位于 shell。把 Pager Markdown renderer 导入 shell 会造成逆向依赖/环。应由 Pager 的 `trajectory_cmd` 组合层向现有 serve 入口注入一个窄的导出回调，Shell 提供固定 session 的只读快照，Pager 在 worker 内完成展示投影与打包，返回拥有生命周期的下载产物。

RenderBlock 内有 `!Send` 的高亮状态，`dispatch_open_transcript_pager` 已说明该限制。应把 Send 的原始快照/通知传给 worker，在 worker 内创建、使用和销毁 tracker/blocks，不能先在 UI 线程构建它们再跨线程发送。

## 6. replay 的证据与限制

- `SessionUpdateEnvelope.timestamp` 是写盘秒级时间；通知 meta 可有 `agentTimestampMs`、`streamStartMs`、`turnStartMs`；`TimelineEvent.at_ms` 是毫秒级事实时间。
- `response_projection.rs::project_admitted_response` 保存完整 assistant/reasoning，`into_notifications` 再生成 chunk。durable candidate 仅留首个无正文锚点；原始 token 边界和到达节奏不是可完整恢复的数据。
- `storage::projection_envelope` 经 `SessionUpdateEnvelope::from_update` 合成记录，会填当前时刻。这个值不能作为历史播放时间；来源 provenance 必须在 reconciliation 前保留并传到展开事件。
- `plan_response_projections` 以 candidate anchor 放回 canonical response，discarded/quarantined/rewound candidate 不能通过模拟流式重新出现。
- `NotificationMeta.is_replay` 表示恢复历史。很多 handler 会因此压制瞬态进度；它不是“可播放但无副作用”的完整能力模型。不能靠把这个 bool 改成 false 恢复动画。
- 普通 `app::run` 建连接、验证模型配置、恢复 session；`AppView` / `AgentView` 同时持有输入、权限、草稿、任务控制等状态。replay 应使用 ScrollbackPane、RenderBlock、layout/motion 等展示组件及窄的公共投影 helper，不能启动整个执行 app 然后隐藏输入框。
- `motion::FrameStamp` 支持给定 Instant 的时间样本；`SubagentInfo::display_elapsed_at` 已有注入展示时间的接口。历史业务耗时与当前终端 UI 动画应分开，不用真实 now 减历史 timestamp。

## 7. 本轮没有作出的承诺

没有声称逐 token 原始录像可恢复，没有声称跨多个活跃 ledger 有全局原子快照，没有声称已保留全部瞬态 UI 数据。导出和 replay 的一致性以同一已捕获、可验证的会话展示快照为准；缺失 transient 数据不能虚构成历史事实。

本轮没有审计所有 live handler，也不以 Atlas 局部 caller 结果证明全仓库完整性。后续实现须按 design 的事件覆盖矩阵逐项核对真实 handler 和 fixture；相关测试尚未运行。
