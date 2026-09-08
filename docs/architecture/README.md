# 架构阅读顺序

这里保留给开发者看的机制解释。行为要求与验收场景以 [OpenSpec](../../openspec/README.md) 为准；既有文章中的旧版本号和修复过程不能覆盖当前规范。

| 阅读材料 | 关注点 | 对应规范 |
| --- | --- | --- |
| [Agent Timeline](agent-core-timeline.md) | 事实、Surface 与请求投影 | [session-timeline](../../openspec/specs/session-timeline/spec.md) |
| [输入路由](input-routing.md) | 输入接纳、FIFO 与前台所有权 | [input-admission](../../openspec/specs/input-admission/spec.md) |
| [Behavior](behavior-state-overview.md)、[Goal](goal-continuation.md) | 协作协议与跨 turn 续跑 | [behavior-goal](../../openspec/specs/behavior-goal/spec.md) |
| [压缩](compaction-pre-prune.md) | 冻结输入与边界提交 | [context-compaction](../../openspec/specs/context-compaction/spec.md) |
| [Workflow](workflow-workspace.md) | Definition、Run 与持久化 | [workflow-execution](../../openspec/specs/workflow-execution/spec.md) |
| [本机协调](local-coordination.md) | peer、询问与取消 | [local-coordination](../../openspec/specs/local-coordination/spec.md) |
| [Pager Motion](pager-motion.md)、[链接](pager-hyperlinks.md) | 展示层机制，具体细节尚未穷举为规范 | [client-surfaces](../../openspec/specs/client-surfaces/spec.md) |
| [依赖 workaround](dependency-workarounds.md) | Cargo patch 保留原因 | [模块与覆盖范围](../../openspec/baseline.md) |

大致调用顺序是 `cli → pager/ACP → shell SessionActor → chat-state / sampler / tools`。`chat-state` 持有可重放事实，`shell` 决定何时接纳、运行和提交，`pager` 展示投影；具体依赖由各 crate 的 Cargo.toml 和实现确认。

历史审计与修复文章仍可通过原路径查阅，但新过程记录只写入 OpenSpec change。详情见 [历史资料登记](../../openspec/baseline.md#历史资料)。

客户端授权缓存的标识映射与共享回退见 [tool-authorization](../../openspec/specs/tool-authorization/spec.md#requirement-stable-per-client-permission-cache-keys)。

权限缓存读取错误的默认状态与共享回退边界见 [tool-authorization](../../openspec/specs/tool-authorization/spec.md#requirement-permission-read-errors-do-not-import-shared-grants)。

权限缓存的普通文件与 1 MiB 读写预算见 [tool-authorization](../../openspec/specs/tool-authorization/spec.md#requirement-permission-cache-io-is-bounded)。

调试日志 latest 链接的临时文件归属见 [client-surfaces](../../openspec/specs/client-surfaces/spec.md#requirement-debug-latest-link-updates-preserve-unowned-temporaries)。

调试路由并发创建 writer 的 guard 归属见 [client-surfaces](../../openspec/specs/client-surfaces/spec.md#requirement-debug-routing-retains-only-selected-writer-guards)。

调试日志清理与活动 writer 的文件锁协作见 [client-surfaces](../../openspec/specs/client-surfaces/spec.md#requirement-debug-pruning-respects-live-cooperating-writers)。

统一日志裁剪的尾部读取预算见 [client-surfaces](../../openspec/specs/client-surfaces/spec.md#requirement-unified-log-trimming-reads-a-bounded-tail)。

统一日志 writer 的句柄身份与路径恢复见 [client-surfaces](../../openspec/specs/client-surfaces/spec.md#requirement-unified-writer-identity-belongs-to-the-opened-handle)。

附件正规化的进程级计算并发与取消语义见 [client-surfaces](../../openspec/specs/client-surfaces/spec.md#requirement-image-normalization-workers-have-process-wide-admission)。

图片资产批次失败的回收边界见 [input-admission](../../openspec/specs/input-admission/spec.md#requirement-failed-image-asset-batches-reclaim-completed-files)。

图片资产重试通过有序内容批次目录复用；临时准备、校验和发布归属见 [input-admission](../../openspec/specs/input-admission/spec.md#requirement-image-asset-preparation-is-retry-safe)。

图片描述的 provider 请求使用绝对恢复截止时间；准备阶段消耗预算，持久化收尾仍保留原生命周期，见 [model-sampling](../../openspec/specs/model-sampling/spec.md#requirement-image-description-provider-work-respects-recovery-deadline)。

图片描述的持久化复用按当前未缓存图片组批量查询，一轮只加载和校验一次历史；完整历史在 provider 请求前释放，见 [model-sampling](../../openspec/specs/model-sampling/spec.md#requirement-image-description-recovery-batches-durable-lookups)。

自动命名失败时，备用标题保留前十个词并限制为 Timeline 接纳的 160 个 Unicode 字符；见 [session-timeline](../../openspec/specs/session-timeline/spec.md#requirement-fallback-session-titles-fit-canonical-title-bounds)。

自动标题通过已接纳 prompt index 定位唯一真实用户消息的 SurfaceId，后续通知或控制事件不会替代其来源；见 [session-timeline](../../openspec/specs/session-timeline/spec.md#requirement-automatic-title-provenance-identifies-admitted-user-input)。

直接终端命令没有 prompt index，其标题来源使用持久化输入提交返回的事件 ID；同样遵循上述标题来源契约。

自动命名配置在 ready、claimed、closed 间转换，手动改名后的关闭状态不会被后台失败回调恢复；见 [session-timeline](../../openspec/specs/session-timeline/spec.md#requirement-manual-title-route-revocation-survives-background-failure)。

会话重命名从查找到标题提交都持有同一生命周期锁，与加载、删除串行；持锁期间直接检查在线 actor，避免等待排在锁后的加载，见 [client-surfaces](../../openspec/specs/client-surfaces/spec.md#requirement-session-rename-participates-in-lifecycle-serialization)。

会话 TTL 清理在配置无效或截止日期无法表示时停止，不把错误配置折算为更短保留期；未配置仍为 30 天，见 [client-surfaces](../../openspec/specs/client-surfaces/spec.md#requirement-session-cleanup-ttl-cannot-wrap-or-overflow)。

TTL 清理在取得写入租约后重新读取并校验同一会话的活动时间，无法确认当前过期状态时保留会话；见 [client-surfaces](../../openspec/specs/client-surfaces/spec.md#requirement-session-cleanup-rechecks-activity-under-writer-ownership)。
