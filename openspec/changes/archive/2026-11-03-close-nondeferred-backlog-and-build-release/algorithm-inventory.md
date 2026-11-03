# Backlog 核对清单

本文件为 umbrella change 的只读源码盘点，不代表算法候选已实施或性能验证已通过。以下算法现状来自当前工作树源码；本盘点未运行 Cargo、测试、基准、合成负载或真实会话。候选的测量门槛是收集证据后决定实施或证据关闭的步骤，不自动构成延期。源码行号用于定位，提交前若文件变化应重新核对。

## 可从活动 backlog 移除的已闭合记录

以下事项已有归档实现/验证证据；可从活动 backlog 移除，归档记录仍保留追溯。

| Backlog 项 | 归档证据 |
| --- | --- |
| Dashboard 与新版会话 UX 对齐 | [实施与验证](../2026-11-03-align-dashboard-session-ux/verification.md)、[独立审查](../2026-11-03-align-dashboard-session-ux/review.md) |
| Fork 多 session 删除资格 | [资格验证](../2026-11-02-allow-independent-fork-session-deletion/verification.md)、[预算验证](../2026-11-02-delete-session-without-maintenance-observation-budget/verification.md) |
| Session 删除后的页面状态收敛 | [验证](../2026-11-02-reconcile-session-deletion-with-runtime-and-views/verification.md) |
| Dashboard 停止与 Working 分类 | [验证](../2026-11-02-fix-dashboard-background-stop/verification.md) |
| Fork 失败占位页回收与来源关闭后的身份收敛 | [回收验证](../2026-11-02-retire-empty-failed-fork-placeholders/verification.md)、[来源关闭后收敛](../2026-11-02-preserve-fork-failure-identity-after-source-close/verification.md) |
| A6/A7 runtime close 与退出作用域 | [关闭验证](../2026-11-02-close-dashboard-sessions-through-runtime/verification.md)、[退出验证](../2026-11-02-scope-exit-suspension-to-runtime-owners/verification.md) |
| 挂起独立历史的显式放弃 | [验证](../2026-11-02-allow-explicit-discard-of-independent-suspended-history/verification.md) |
| 子代理委派复审四项 | [验证](../2026-10-31-fix-subagent-delegation-authority/verification.md) |
| 原生 float 错误指针与 RISC-V warning | [验证](../2026-10-31-fix-native-vector-error-initialization/verification.md) |
| Plan 窄屏、Minimal 预览/批注与普通批注 | [旅程修复验证](../2026-10-31-audit-plan-user-journey/verification.md) |
| Runtime suspend 等待已 park foreground | [验证](../2026-10-31-fix-suspension-compaction-drain/verification.md) |
| Agent 禁用归因、诊断和 toggle 同步 | [验证](../2026-10-31-fix-agent-authorization-and-toggle-reload/verification.md) |
| ACP watcher 注册阻塞及注册间同步缺口 | [验证](../2026-10-29-unblock-acp-during-watcher-registration/verification.md) |
| LSP refresh mock 固定 sleep | [验证](../2026-10-29-synchronize-lsp-refresh-test/verification.md) |
| SideQuestion 接收者关闭后仍运行 | [验证](../2026-10-29-cancel-abandoned-side-questions/verification.md) |
| 不可验证 child 完成被记为成功 | [验证](../2026-10-29-require-canonical-unavailable-child-result/verification.md) |
| release 运行 debug-only panic 测试 | [验证](../2026-10-29-clarify-committed-anchor-release-test/verification.md) |
| Chrome duration 缺计时 span | [验证](../2026-10-29-emit-chrome-instrumentation-timers/verification.md) |
| sticky 关闭/compact 导航几何不一致 | [验证](../2026-10-29-fix-pager-sticky-navigation-geometry/verification.md) |
| 后代展示跨实体校验与无 ID 重入 | [验证](../2026-10-29-fix-descendant-presentation-replay/verification.md) |
| tmux post-exit 清理预算区分 | [验证](../2026-10-29-test-tmux-post-exit-cleanup-budget/verification.md) |
| Git/worktree 构建身份监听 | [验证](../2026-10-29-fix-build-git-identity-invalidation/verification.md) |
| PTY daemon fixture owner | [验证](../2026-10-29-own-pty-leader-test-daemons/verification.md) |
| OCR 后代及并发 fixture 失败清理 | [验证](../2026-10-29-own-local-ocr-process-tree/verification.md) |
| Plan handoff 已消费后误报 ACK 丢失 | [验证](../2026-10-29-fix-plan-handoff-consumption-recheck/verification.md) |
| Goal Control 首请求前 scope 同步 | [验证](../2026-10-29-fix-goal-control-context-scope/verification.md) |
| 统一日志溢出测试时序假设 | [验证](../2026-10-29-synchronize-unified-log-overflow-test/verification.md) |
| 活动组 PTY 分组边界和等待 | [验证](../2026-10-29-align-activity-group-pty-contract/verification.md) |
| Runtime 回归进入 Core 门禁 | [验证](../2026-10-29-gate-backlog-runtime-regressions/verification.md) |
| Native 分段与 AgentMessage 交错候选（证据关闭） | [通路审计](../2026-10-29-audit-runtime-native-message-tool-batch-boundary/audit.md) |
| Markdown clone 候选（证据关闭） | [调用归因](../2026-10-29-fix-pager-sticky-navigation-geometry/audit.md) |
| 压缩、计数与模型切换边界 | [状态验证](../2026-10-29-fix-compaction-pressure-lifecycle/verification.md)、[组合故障注入](../2026-10-29-verify-compaction-publication-boundaries/verification.md) |
| 真实持久化与进程中断恢复 | [验证](../2026-10-29-verify-compaction-process-recovery/verification.md) |
| 低值 provider 校准、输出预留与组合上下文转换 | [边界验证](../2026-10-29-resolve-context-token-boundaries/verification.md)、[组合验证](../2026-10-29-verify-composed-context-transitions/verification.md) |
| 算法 R1 Timeline admission | [验证](../2026-10-21-optimize-timeline-admission-validation/verification.md) |
| 算法 R2 response projection append plan | [验证](../2026-10-21-reuse-response-projection-append-plan/verification.md) |
| 算法 R5 writer 首次 Timeline append | [验证](../2026-10-21-optimize-timeline-writer-prefix/verification.md) |
| 算法 R6 同次恢复 canonical 复用 | [验证](../2026-10-21-reuse-resume-canonical-projections/verification.md) |
| 算法 P0/P1/P2 Pager presentation | [P0 归因](../2026-10-22-profile-resume-presentation-attribution/verification.md)、[P1 延后历史 Markdown](../2026-10-22-defer-historical-markdown-presentation/verification.md)、[P2 后代恢复复用](../2026-10-22-reuse-validated-descendant-recovery/verification.md) |
| 算法 U1/R4 Pager resume 布局 | [验证](../2026-10-21-optimize-replay-layout-maintenance/verification.md) |
| 算法 S1 exact commit 索引 | [验证](../2026-10-21-index-exact-update-commits/verification.md) |
| 历史遗留缺陷与测试债务（原 2026-10-29 清理） | [完整发布验证](../2026-10-29-close-backlog-and-release-patch/verification.md) |

以上已闭合事项不能外推为相邻恢复、Plan、MCP 或性能候选也已闭合；后者在下表单独保留。

## 非暂缓、仍待落实的事项

| 事项 | 当前证据与必须核对的权威来源 | 当前处置 |
| --- | --- | --- |
| 恢复准入 S1：规则来源同源 | 方案：[恢复准入设计](../2026-11-02-analyze-session-recovery-admission/design.md)、[验证计划](../2026-11-02-analyze-session-recovery-admission/validation-plan.md)。检查 `shell/src/agent/prompt_build.rs`、`agent/subagent/handle_request.rs`、`agent/mvp_agent/runtime_handoff.rs` 与 source hash 的来源；权威契约 `specs/session-timeline/spec.md`、`configuration-rules/spec.md`、`tool-authorization/spec.md`。 | 实施并验证恢复使用持久 ProjectInstructions 的同一投影，不能宽泛跳过 source hash。 |
| 恢复准入 S3：失败 load 的同进程 retry / owner | 设计、验证计划同上。检查 `shell/src/session/persistence.rs` 的 load、`agent/mvp_agent` 的树 restore/release、actor 临时 owner 生命周期；权威 `session-timeline` 与 `tool-authorization`。 | 验证 failure phase、actor retirement/reuse、writer 与 prepared/released instance 所有权；按方案实现。 |
| 恢复准入 S2：执行依赖、workspace 身份、出生 MCP | 设计同上；`agent/subagent/handle_request.rs`、`session/persistence.rs`、`agent/mvp_agent/runtime_handoff.rs`、`session/storage/jsonl/mod.rs`。权威 `session-timeline`、`tool-authorization`、`sandbox-boundary`、`extension-runtime`。 | 分清展示/路径无关漂移与真实执行来源漂移；核验普通 Control/child recipe 是否足以恢复出生 ceiling。不能将 hash gate 裸删。 |
| `resume_from` 派生 child 的继承展示 | backlog 原证据指向 `shell/src/session/persistence.rs:2935` (`new_child`) 与 `session/storage/jsonl/mod.rs:4629` (`init_session_with_summary`)，对照 `copy_session_data` 及历史 projector；权威 `session-timeline`、`client-surfaces`。 | 验证 seed 中源 Surface 是否被持久化为新 lineage 的 presentation facts、冷恢复是否展示；设计并闭合差异。 |
| workspace 路径别名恢复 | 复现证据：[子代理权限验证](../2026-10-31-fix-subagent-delegation-authority/verification.md)。检查 `agent/subagent/handle_request.rs` cwd 解析和恢复 source hash；权威 `sandbox-boundary`、`tool-authorization`、`session-timeline`。 | 统一 workspace 身份入口并以 `/tmp`/`/var` alias 冷恢复覆盖；不能移除 source hash 来绕过。 |
| 挂起 child 出生 MCP 成员集合 | 上述验证 `verification.md` 的“独立发现”说明跨进程 MCP pool 为空，live 测试不证明冷恢复 ceiling。检查 Spawn/child restore 与 MCP pool 重建；权威 `extension-runtime`、`tool-authorization`。 | 测“先窄继承→切宽 profile→挂起→新进程/新 incarnation 恢复”；根据结果修复或用证据关闭。 |
| ACP Plan malformed approval response | [Plan 旅程审计](../2026-10-31-audit-plan-user-journey/audit.md) §外部 client；检查 `shell/src/session/actor/tool/mod.rs` 的 approval lifecycle 与 ACP response parse/dispatch；权威 `task-planning`、`client-surfaces`、`workflow-execution`。 | 定义坏响应后的同连接恢复行为和失败语义；不增加无界重试。 |
| Plan 恢复 artifact 二次读取竞态 | 审计同上；`shell/src/session/actor/tool/mod.rs:1133,1286–1327`。 | 对 artifact 二读失败与当前 Plan 替换做故障注入，证明旧任务只影响 exact snapshot；`待故障注入`是验证任务，不是延期。 |
| Sampler 日志捕获偶发失败 | 证据：[兼容性验证](../2026-10-29-improve-cross-route-reasoning-compatibility/verification.md)，backlog 所述 `sampling_auth_logs_omit_credentials` 的 `logs.contains("client_post")` 曾失败一次。 | 隔离并复现测试环境/并发路径，之后修复或以稳定复现记录证据关闭；“待定位”不是延期。 |

## 算法候选当前实现、入口和最小验证

本节对应 [底层算法审计](../2026-10-21-audit-grow-algorithmic-hotspots/audit.md) 和五份分报告。下列现状是源码事实，不是本次执行过的测量。候选实现需保持审计列明的语义和对应 `openspec/specs/` 契约。

| 候选 | 当前源码与状态 | 既有验证入口 | 最小验证/测量门槛 |
| --- | --- | --- | --- |
| R3 JSONL / replay 表示 | 主要候选已实现：当前 reader `shell/src/session/storage/jsonl/mod.rs:2484–2555,2578–2638` 用 `RawValue` 取记录、窄 `VersionProbe` 先判 schema、再直接 typed decode；无完整 `Value` 中间树。生产 replay `shell/src/agent/mvp_agent/mod.rs:1144–1170` 从 `HistorySnapshot::prepare` 得到 typed updates，再直接转发 ACP notification；旧 `forward_raw_replay_line` 是 `#[cfg(test)]` 路径。剩余每条 versioned JSONL 记录仍有 header probe + typed decode 两次 parse，见下方 R3 复核。 | [streamline-resume verification](../2026-10-21-streamline-resume-codec-and-projections/verification.md)；`shell/src/session/storage/jsonl/tests.rs::load_reports_the_persisted_and_expected_timeline_schema_versions`、`::sideband_schema_is_checked_before_event_shape`；`shell/tests/session_load_perf.rs::phase_breakdown_real_functions`；`pager/src/app/acp_handler/tests/semantic_replay_bench.rs::semantic_replay_without_drawing_scales_at_128_512_2048_turns`。 | 不因残留双 parse 直接改协议或引入缓存。若需要判断成本，用现有 phase benchmark 先测 `updates read+parse+filter`，并用内部临时计时隔离 version probe 与 typed decode；保留未来版本优先报错、行号、torn tail、最大行长度和 session identity。 |
| S2 search projection | `shell/src/session/storage/search.rs:1045–1070,1466–1472` 每次 upsert 仍全读 Timeline、`Timeline::from_events`、重建全文；content hash 只省索引写入。未见后续增量投影。 | 同文件 `timeline_collector_reports_bytes_and_validates_lifecycle` `:1868`、pinned entity `:1888`、append capture `:1925`、bootstrap recheck `:2041`、failure repair `:2162`。 | 对 title-only/无关事件/正文追加/rewind/compaction 做增量与全量结果差分；128/512/2048 events 测读取字节、fold、RSS；覆盖冷 bootstrap 与实时交错。 |
| S3 session list | `shell/src/session/listing.rs:98–121,158–164` 仍先构造所有 `SessionListing` 和时间字符串，再排序并解析时间，去重后截断。 | `listing.rs:229 listing_filters_and_keeps_only_newest_empty_session_per_cwd`；已有 `shell/benches/session_list.rs`（约 9,864 summaries）可作基线。 | 保持 query→排序/tie→cwd 去重→limit 的完全差分；覆盖 same cwd、tie、缺 last-active、limit=0；多集合规模 benchmark。 |
| C1 图片 SurfaceId 查找 | `chat-state/src/actor/request_builder.rs:94–106,237–249` 验证与应用替换各线性查找 SurfaceId，仍可见。 | `request_builder.rs:580–910` image budget tests；`actor/tests.rs:8489,8744+` key 与 selection 重用。 | 多 Surface×多替换与索引实现做结果差分；覆盖 stale ID/part mismatch/淘汰；测 N、K 增长。 |
| C3 prompt cache rewind anchor | `chat-state/src/actor/request_builder.rs:260–278` 每次 key 构建反向扫描 Timeline events。 | `chat-state/src/actor/tests.rs:8489 prompt_cache_key_tracks_timeline_model_and_rewind_lineage`。 | 大量普通 append 后 key 稳定；已提交 rewind 更新锚点，失败/未确认 rewind 不更新；测事件数×请求数。 |
| C2 历史损坏修复 | `chat-state/src/compaction_utils.rs:1165–1171` 多坏 exchange 逐次反向 splice；`sampling-types/src/conversation.rs:4825–4829` dangling repair 多次 splice，`:4938–4945` duplicate result 多次 `Vec::remove`。`strip_displaced_tool_results` (`compaction_utils.rs:1185–1209`) 已用 retain 单次扫描，故该子路径已解决。 | `compaction_utils.rs:1313,1369,2967` malformed/duplicate tests；`sampling-types/src/conversation.rs:10587+` dangling repair tests。 | 大量交错且互不相邻的 malformed/dangling/duplicate 输入，比较新旧结构、顺序和隔离证据范围；仅对损坏路径测 N/K 成本。 |
| C4 request integrity clone | `chat-state/src/actor/mutations.rs:173–184` 仍在修复前克隆完整 Surface；健康路径丢弃副本，随后 request builder 另克隆请求内容。 | `chat-state/src/actor/tests.rs:5840–5970,6428–6448` crash/cancel dangling repair。 | 验证候选副本复用且健康投影等价；修复需持久化确认，失败时不能返回部分修复请求；测分配/克隆量。 |
| T1 PerDir 初始 watch 选择 | `fsnotify/src/watcher.rs:657–672` 仍全量收集并按深度稳定排序，`:1064–1077` 才截 watch budget。 | `watcher.rs:3296,3331,3347,3369`；`fsnotify/benches/startup.rs` 已有启动基准。 | 截断前后选集差分，保持 ignore/glob、浅层优先、同深度 DFS 顺序；大于 budget 合成树测峰值内存与耗时。 |
| T2 多 hunk patch | `hunk-tracker/src/actor/queries.rs:38–67` 每个 hunk 调 patch；`diff.rs:81–84` 每次为 before/current 重建整文件行表。 | `diff.rs:777–831` patch tests；`actor/tests.rs:2950–3200` GetFileHunkData 状态覆盖。 | 多 hunk 样例完整 patch 字节差分；接近 1 MiB、高 hunk 数测试行表重复成本。 |
| T3 workspace metadata | `workspace/src/file_system/walk.rs:104–115` 每项先 `symlink_metadata` 后无条件 `metadata`；confined symlink filter `:89–90,125–132` 另可能探测。 | `file_system/client_fs.rs:382–424,449–491` 排序/分页/root/symlink 回归。 | 普通文件/目录复用 lstat metadata；symlink 仍报告链接身份并使用目标 metadata；验证根约束、删除竞态，测大目录 syscall 数和时间。 |
| T4 工具 manifest clone | `shell/src/session/actor/turn/sampling.rs:789–857` 每 step 拉取 owned definitions 并动态过滤；`tools/src/registry/types.rs:1398–1404` 深克隆 definitions，sampling `:780–784` 再建 ToolSpec。 | `shell/src/session/actor/tests/rewind_cross_compaction_tests.rs:552–609`；`registry/types.rs:3577` MCP/builtin lifecycle。 | 先计每 step schema/description 字节与构造时间；若成本可测才评估 immutable snapshot；用现有行为切换/MCP 更新用例验证及时性。 |
| U2 session picker 派生显示数据 | `pager/src/views/welcome/mod.rs:660–680` 和 `app/agent_view/modal_routing.rs:2118–2130` 每次绘制重新 filter/build 全部匹配项；`views/session_picker.rs:336,583` 分别实现全量过滤与 owned 派生。 | `views/session_picker.rs:883–1169` query/group/content/header/index mapping tests。 | 先测实际 S 分布和小/大 S paint 成本；不变帧、query、宽度、展开、content arrival、repo/time 标签变化都与完整 map/selection 对照。 |
| U3 dashboard child top-8 | `pager/src/views/dashboard/row.rs:197–266` 仍收集并稳定排序全部非 workflow children 后才截 8。 | 同文件 `:1201` workflow 过滤、`:1263` cluster；`app/root/dispatch/tests/dashboard.rs` 与 dispatch test cap 场景。 | M<8/=8/>8、running优先、equal timestamp来源顺序、workflow过滤、pinned child、more总数与完整排序差分；再按真实 M 或合成高 M 测量。 |

### R3 复核：残留双 parse 的行动边界

- 归档 [R3 优化](../2026-10-21-streamline-resume-codec-and-projections/design.md) 已将 version 读取改为窄 envelope，并让 Timeline kind 直接 typed decode；projection 通知复用 typed 转发入口。后续 [统一 Timeline/history 验证](../2026-10-21-streamline-resume-codec-and-projections/verification.md) 记录 malformed/version/size/torn-tail 与 duplicate typed field 回归通过。`git log` 显示这两处现状由 `567560b82 feat: prepare 2.5.0 with unified timeline and resume optimizations` 引入；当前 `forward_raw_replay_line` 带 `#[cfg(test)]`，生产 `replay_session_updates` 不再经过 typed→JSON→raw parser 的递归路径。
- 仍需 version probe 的权威理由在 [session-timeline spec](../../../specs/session-timeline/spec.md) 的 “Timeline wire decoding rejects ambiguous typed fields” requirement 和 “A future schema has an unknown shape” 场景：任意合法字段顺序必须可读，且不兼容 schema 必须先报告精确 persisted/current mismatch，即使未来 payload 缺失 kind 或形状未知。当前 [测试](/Users/lordcasser/workspace/projects/grow/crates/codegen/shell/src/session/storage/jsonl/tests.rs:4058) 把 version 改为未来版本、添加未来字段并删除 `kind`，断言先返回版本 mismatch；Sideband 同样有 `sideband_schema_is_checked_before_event_shape`。一个先解完整 typed 对象再检查版本的实现会改变这个错误契约。保留 wire 序列化和双 parse 符合当前契约；不应仅为把扫描数降为一而造 parser、缓存或改变字段顺序要求。
- 因此原 R3 两项实现债务（完整 Value 树、production replay roundtrip）已闭合；残留双 parse 不是有证据支持的待实现项，只是未单独归因的测量假设。历史 release profile 测得 Timeline read/decode 110.395→67.418 ms、forward updates 63.406→22.917 ms，但这是组合阶段，无法把收益或成本归给 version probe；历史性能文件已清理，verification 只保留汇总结论。当前没有执行任何测量。
- 可复跑的现有粗粒度入口是 `cargo test --locked -p shell --features test-support --test session_load_perf phase_breakdown_real_functions -- --nocapture`；对应指标是 fixture 的 updates read+parse+filter 墙钟时间。`cargo test --locked -p pager --lib semantic_replay_without_drawing_scales_at_128_512_2048_turns -- --ignored --nocapture` 提供 Timeline decode/project 与 handler replay 的更大端到端窗口，不隔离 version probe。若粗粒度结果值得追查，再用临时内部计时在 `validate_record_version` 与随后的 typed deserialize 两段分别记录 CPU 时间/分配，并配对同一 build/fixture 比较；这属于新增诊断测量，不是已有验证。

算法审计要求共同的语义差分与规模曲线（旧/新同一 fixture、输出和错误行为一致；128/512/2048 或适用规模），详见 [audit.md](../2026-10-21-audit-grow-algorithmic-hotspots/audit.md#L62)。历史审计没有本次验证结果；不把历史基线冒充当前实现测量。T4、U2、U3 及低频 C2/C4 的基准决定其收益，不取消用户对非延期 backlog 的授权；测后如无收益，以证据关闭。

## 明确暂缓 / 等待条件的事项

| 项目 | 保留依据与启动条件 |
| --- | --- |
| 跨路由 reasoning 更强语义 | 原 backlog：“若真实目标要求更强语义，应独立处理精确 continuation 拒绝分类或最小 provenance。”只有产品目标提出更强语义时启动。 |
| Remote Control / Agent Host 跨设备 | 原 backlog：“暂缓、未排期”“不实施、不设日期”；未来启动时另建 change，并先明确身份、权限、执行 owner、断线和恢复边界。 |
| Provider 缓存/native epoch | 原 backlog：“先取得具体 provider/model/deployment、字段透传和真实负载证据，再做该 route 的最小优化。”这是可观察的先决证据，不是永久搁置。 |
| Recap 与主请求缓存 | 原 backlog：“先测真实收益。”测量后实施最小优化或证据关闭；保持 recap 的独立 key 与 tool-free Sideband 契约。 |
| 跨多个合法选区连续摘要 | 原 backlog：“契约选择，未启动”；当前一次有界摘要后超窗即按既有契约终止。若要继续压缩，先独立建立新契约。 |
| MCP 多轮 elicitation | 现有 active proposal：[tasks](../../add-mcp-multi-round-elicitation/tasks.md)、[SDK 接点核查](../../add-mcp-multi-round-elicitation/sdk-integration.md)。官方 SDK 尚缺调用级 metadata、整轮预检、逐轮取消/期限、等待与派发前 gate。继续条件：先由官方 SDK 补齐/暴露接点并通过 fixture，再接 Grow owner 与 UI；上游条件未满足前暂缓。 |

“待专项验证”“待故障注入”“待定位”均未列入本节：它们是本 change 的验证工作，不能解释为延期。

## 本轮 T3 已实施

见 [普通项 metadata 复用](../2026-11-03-avoid-workspace-entry-metadata-dup/verification.md)：公开目录操作17项测试通过，symlink目标、失效链接、confinement及分页保留；普通项省去第二次follow-stat，保持已有lstat失败fallback。两次独立测试进程、等价10k文件fixture的本机debug探针中位54.093ms→42.654ms，无显式预热、OS缓存未控制；不声称生产吞吐或symlink-heavy收益。主代理核对diff与文档后归档，strict全量18/18、archive783/783通过。

## 本轮恢复与展示已闭合

| 原待落实事项 | 最终证据 |
| --- | --- |
| S1持久规则同源 | [归档](../2026-11-03-restore-persisted-project-instructions/verification.md)；新child Seeded与旧Existing区分，真实provider/classifier输入。 |
| S3失败load同进程retry | [归档](../2026-11-03-reload-unprepared-failed-session-restores/verification.md)；物理退休原owner，后续S2以缺MCP真实依赖继续验证同manager retry。 |
| S2出生MCP | [归档](../2026-11-03-persist-child-birth-mcp-membership/verification.md)；出生grant与实际pool同源，current authority求交。 |
| S2执行依赖/目录别名/实际模型路线 | [归档](../2026-11-03-admit-session-restores-by-execution-dependencies/verification.md)；root5阶段、child30阶段，strict身份、不按metadata digest拒绝、原role/rules及显式reselect、fresh child自身route baseline。 |
| Task resume_from继承展示 | [归档](../2026-11-03-preserve-task-resume-inherited-transcript/verification.md)；真实Completed/Cancelled source→resume→live/cold，完整工具更新、源bytes不变、同cut与新Seed引用。Shell全量4473 passed、唯一新fixture错误修正后定向通过，9manual bench ignored，未伪称旧全量命令exit0。 |

本节覆写上方初始盘点的“当前处置”状态；初始盘点保留为追溯，不是未完成队列。其余算法与Plan/Sampler仍按tasks逐项验证，不因本节删除。

## Plan及Sampler后续闭合

- Plan artifact二读竞态及malformed显式恢复：见[实施/红绿故障验证](../2026-11-03-bind-restored-plan-approval-to-one-snapshot/verification.md)，110项Plan+11项reverse request；独立review补齐正文身份错配和stale bit提前发布/回滚覆盖，均验证后修复。不新增审批重试API。
- Sampler历史日志候选：见[调查与诊断](../2026-11-03-diagnose-sampler-auth-log-test-observation/verification.md)。当前全量平行/32线程均未复现；历史失败capture未留存，根因未知。只补失败capture，保留所有wire/log/no-secret断言，没有声称修复flake；按无当前reproducer归档调查结果，原观察与限制保留。

## 算法后续结论

- **R3证据关闭**：原完整Value中间树与production replay JSON往返已由2026-10-21归档优化消除；当前必要的version-first probe不是新实现债务。本轮Shell全库回归中的 `load_reports_the_persisted_and_expected_timeline_schema_versions`、`sideband_schema_is_checked_before_event_shape`均通过，保留未知未来形状优先报告版本差异的正式契约。未测/未声称probe单独收益；不为扫描次数造parser/cache。
- **S3已实施**：[typed key与延后行构建](../2026-11-03-defer-session-listing-materialization/verification.md)。独立review保留Chrono扩展年份旧parse边界与稳定tie、query/empty dedup/limit；最终差分2项、unified filters/facets/cursor3项通过。debug纯阶段9864summary旧reference中位14.937ms→2.782ms；不等同磁盘或Pager端到端提速。

- **C1已实施**：[复用图片选择offset](../2026-11-03-reuse-image-budget-choice-offsets/verification.md)。没有新索引，沿用完整saved Surface IDs和Weak身份校验；ChatState全量603 passed/3既有manual bench ignored。8192项尾部1图validation+apply中位46.5µs→0.375µs，64图3.133ms→13µs；前部少量选择持平或小幅波动，不声称整请求收益。

- **C3已实施**：[accepted rewind anchor派生投影](../2026-11-03-cache-last-rewind-anchor/verification.md)。只在共同validated apply更新，Default/Clone/replay/prepare/失败accept与原scan一致，完整key字节差分通过；全ChatState604 passed/4 ignored（其中临时probe已单跑通过并从build移除）。8192events无/旧rewind全key中位约66µs→2.6–2.9µs，最新rewind持平；不是端到端请求测量。

search S2及task9候选仍继续按当前tasks处理。

## 实测后关闭 T4 / U2 / U3

见[完整成本门槛、源码与原始样本](cost-gates.md)：T4当前raw30工具schema25,151B+description13,876B，registry owned clone中位0.191ms，最终27工具完整prepare+spec0.368ms/step；没有证据值得改变动态registry/权限/MCP失效边界，按现负载关闭共享manifest候选，不宣称任意MCP规模成本小。U2已逐入口核对默认30：collapsed派生6.7µs，真实Welcome/Agent picker绘制0.501/0.216ms，全展开0.582/0.340ms。100/128/512/2048压力值完整保留，不造selection/query/time/width失效缓存。U3全child stable top8仅test调用；production统一roster入口include_subagents=false，不在该排序路径。生产2048child行构造约42µs，旧kernel334µs不应混归给它。三项没有生产行为更改，临时探针删除后SHA256复原原状态，3+1定向探针通过。

## T2已实施

[同文件多hunk行表复用](../2026-11-03-reuse-file-line-tables-for-hunk-patches/verification.md)：保留single API和原formatter，actor以zip移动patch，无跨查询cache；完整字节差分与实际FileContentState边界通过。全hunk-tracker200 passed/1既有ignored，独立review通过。近1MiB/8hunks同进程旧reference10.762ms→shared1.349ms；单hunk近似持平。fullwalker/UI/OS IO无该收益声明。归档后strict archive792/792通过。

## 搜索S2已实施

[validated prefix复用](../2026-11-03-reuse-validated-search-ledger-prefix/verification.md)：仅worker最近一次成功projection，旧complete_len全字节hash证明后用统一record scanner读取suffix，消费owned Timeline的replay入口保留错误顺序；failed candidate丢弃且旧SQLite row保留，成功才回存。实体替换/同尺寸且mtime不变改写/截短/fresh torn snapshot补全/混合Presentation→Domain和重复eventId、compaction/rewind、checksum/schema/seq/physical-vs-lifecycle及同event content-vs-lifecycle都与旧full差分。独立review修正blocking正文提取和miss先释放旧fold。

最终ChatState604 passed/3ignored，Shell4485 passed/9ignored，无失败；全量运行中追加的Control双错误断言另做最终exact1/1通过。临时计数器/probes移除且lib SHA256复原。2048turn/3.17MB同fixture debug旧full313.26ms，cold311.40ms，unchanged46.57ms，append1turn47.10ms，rewrite fallback347.67ms；retain4,205,664 Rust requested live bytes、drop回baseline。没有O(1)/RSS/真实负载/端到端承诺，idle仍保留最后session全Domain history+AppendState；key交错会cold，不加字节cap/mtime信任/增量text。

## C2已实施

[修复编辑一次合并/retain](../2026-11-03-rebuild-history-repair-edits-once/verification.md)：原范围/插入判定和run内last-wins不变，完整旧JSON与五字段report/幂等差分；三种reason、SkillLoad边界、相邻malformed及private reasoning隔离。sampling-types320 passed，ChatState606 passed/3既有ignored，独立review通过。8192/K512插入4.58ms→0.359ms、删除4.57ms→0.434ms；K1隔离63µs→160µs、删除40µs→139µs，未作uniform-speed承诺/阈值分支。quarantine全K证据和dangling新旧数组暂共存空间代价明确，无heap/RSS声称。

## C4已实施

[healthy owned snapshot复用](../2026-11-03-reuse-healthy-request-integrity-snapshot/verification.md)：healthy Some复用、changed ACK后None再clone，push-user丢副本不增复制；MemoryContext ACK后同typed item入owned请求，Goal/image/native后续不变，无缓存/另一检测器。真实request-stage raw dangling→repair失败exact retry→memory ACK→完整request/canonical/cold相等，两取消点不返回请求，Goal shadow仅request/相同memory不再写。最终ChatState608 passed/3ignored，独立review通过。8192items纯text clone/repair1.842→0.977ms，tool-heavy11.121→10.088ms，image Arc2.121→1.126ms；非整请求/深拷贝图片字节测量。owned Vec跨新Memory ACK可能增加峰值，未作heap/RSS或普遍峰值降低承诺。

## T1已实施

[预算内watch候选](../2026-11-03-bound-initial-per-directory-watch-selection/verification.md)：walker懒消费，先take(B)，不溢出保留稳定depth排序；实际overflow才heapify该buffer，按(depth,source ordinal)保留最浅B，完整total用于原告警。B0/MAX、相邻deep/mixed/cyclic、reverse输入反字典序tie、ignore/symlink/pruning与真实12k候选精确差分。全fsnotify库119 passed/15ignored、integration2 passed/6ignored、doc0，最后reverse断言exact1pass，独立review通过。

结果是候选len≤B与O(min(N,B))保留量，不是分配capacity精确≤B、少IO或普遍提速。最终owned-input typed-old-reference对照包含输入clone/返回drop，default12k mixed1.279→1.628ms、equal0.601→0.835ms；paired typed-reference fullwalker301.0→302.2ms约+0.4%，B8 mixed选择约快25%但fullwalker略慢。原before生产314.6ms为独立run，不能当paired。first堆原型/后续两版raw均保留，不篡改原始数据或混称真正生产前后速度。无byte/RSS测量，具体timer/oracle边界见归档。
