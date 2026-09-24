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

`search_replace` 空匹配创建只在明确 NotFound 后继续；读取失败的边界见 [tool-authorization](../../openspec/specs/tool-authorization/spec.md#requirement-search-replace-creation-requires-confirmed-absence)。

调试 firehose 将所有进程和 session 的事件写入同一个带 `role`、`pid`、`sid` 的日志流；生产端最多排队 64 条完整记录，单条 64 KiB，单文件 32 MiB。后台 writer 在跨进程 inode 锁内追加和保留完整行，路径被替换时重开；退出 flush 最多等待 2 秒。旧的 per-session 文件只做按年龄清理。行为见 [有界流](../../openspec/specs/client-surfaces/spec.md#requirement-debug-firehose-is-a-bounded-attributed-stream) 与 [文件保留](../../openspec/specs/client-surfaces/spec.md#requirement-debug-firehose-retains-a-bounded-complete-line-tail)。

统一日志裁剪的尾部读取预算见 [client-surfaces](../../openspec/specs/client-surfaces/spec.md#requirement-unified-log-trimming-reads-a-bounded-tail)，追加/裁剪锁协调见[统一日志写入契约](../../openspec/specs/client-surfaces/spec.md#requirement-unified-log-appends-coordinate-with-in-place-trimming)。

统一日志单条 JSONL 记录上限为 64 KiB，超限时写入完整的省略诊断行，见 [client-surfaces](../../openspec/specs/client-surfaces/spec.md#requirement-unified-log-records-have-a-bounded-complete-encoding)。

统一日志的磁盘追加由单个后台 writer 执行；生产端使用 64 条有界队列，队列满时丢弃诊断记录并汇总损失数。快照和正常退出最多等待 2 秒刷新，不能把超时或 OS 写入阻塞解释为已落盘。行为见 [client-surfaces](../../openspec/specs/client-surfaces/spec.md#requirement-unified-log-disk-writes-do-not-block-producers)。

统一日志的每次追加会在跨进程 inode 锁内核对 5 MiB 文件上限；必要时先保留最近的完整 JSONL 行，再写入新记录。无法安全裁剪时拒绝本次追加，见 [client-surfaces](../../openspec/specs/client-surfaces/spec.md#requirement-unified-log-appends-enforce-the-shared-file-limit)。

统一日志 writer 的句柄身份与路径恢复见 [client-surfaces](../../openspec/specs/client-surfaces/spec.md#requirement-unified-writer-identity-belongs-to-the-opened-handle)。

Kitty 非 PNG 预览先检查源图片像素预算，macOS `sips` 子进程再限制输出文件大小，Rust 回退编码器限制 PNG 结果缓冲；转换超限时放弃预览，见 [client-surfaces](../../openspec/specs/client-surfaces/spec.md#requirement-kitty-overlay-conversion-bounds-created-output)。

图片查看器对新增的来源副本、转换工作区估算量及保留的原图/显示缓冲使用进程级 896 MB 准入；许可随后台结果或 viewer 所有权移动，关闭与过期结果丢弃时释放。预算不足沿现有预览失败路径结算，不等待 UI；已有 composer 图片不重复计费，图像库额外的内部瞬时分配和 macOS `sips` 子进程内存无法精确计入。行为见 [client-surfaces](../../openspec/specs/client-surfaces/spec.md#requirement-image-viewer-loading-owns-an-aggregate-memory-reservation)。

非 macOS 剪贴板图片的 arboard 读取与 PNG 编码共用单个进程级执行许可，超时的后台 worker 仍持有许可；编码前校验 1600 万像素并限制 PNG 输出为 5000 万字节，Linux 图片 helper 的 stdout 也按同一编码字节预算截断并报错。平台在 `get_image` 返回 RGBA 前的分配仍不受此限制。行为见 [client-surfaces](../../openspec/specs/client-surfaces/spec.md#requirement-non-macos-clipboard-image-work-has-an-in-process-allowance)。

附件正规化的进程级计算并发与取消语义见 [client-surfaces](../../openspec/specs/client-surfaces/spec.md#requirement-image-normalization-workers-have-process-wide-admission)。

Pager 的 macOS 剪贴板元数据探测在单个有界 worker 上执行，前台至多等待 10 ms；后台附件版本核对保留完整原生结果，见 [client-surfaces](../../openspec/specs/client-surfaces/spec.md#requirement-pager-clipboard-metadata-probes-have-a-caller-deadline)。

图片资产批次失败的回收边界见 [input-admission](../../openspec/specs/input-admission/spec.md#requirement-failed-image-asset-batches-reclaim-completed-files)。

图片资产重试通过有序内容批次目录复用；临时准备、校验和发布归属见 [input-admission](../../openspec/specs/input-admission/spec.md#requirement-image-asset-preparation-is-retry-safe)。

图片资产孤儿由 Session writer 在后台对照已提交的物理 Timeline 回收；提交回执丢失时只按实际引用决定保留，见 [input-admission](../../openspec/specs/input-admission/spec.md#requirement-published-user-image-assets-follow-durable-timeline-references)。

图片描述的 provider 请求使用绝对恢复截止时间；准备阶段消耗预算，持久化收尾仍保留原生命周期，见 [model-sampling](../../openspec/specs/model-sampling/spec.md#requirement-image-description-provider-work-respects-recovery-deadline)。

图片描述的持久化复用按当前未缓存图片组批量查询，一轮只加载和校验一次历史；完整历史在 provider 请求前释放，见 [model-sampling](../../openspec/specs/model-sampling/spec.md#requirement-image-description-recovery-batches-durable-lookups)。

自动命名失败时，备用标题保留前十个词并限制为 Timeline 接纳的 160 个 Unicode 字符；见 [session-timeline](../../openspec/specs/session-timeline/spec.md#requirement-fallback-session-titles-fit-canonical-title-bounds)。

自动标题通过已接纳 prompt index 定位唯一真实用户消息的 SurfaceId，后续通知或控制事件不会替代其来源；见 [session-timeline](../../openspec/specs/session-timeline/spec.md#requirement-automatic-title-provenance-identifies-admitted-user-input)。

直接终端命令没有 prompt index，其标题来源使用持久化输入提交返回的事件 ID；同样遵循上述标题来源契约。

自动命名配置在 ready、claimed、closed 间转换，手动改名后的关闭状态不会被后台失败回调恢复；见 [session-timeline](../../openspec/specs/session-timeline/spec.md#requirement-manual-title-route-revocation-survives-background-failure)。

新会话会记录模型路由基线；冷加载若发现当前 catalog 的 provider model 或 transport 与最后一条持久化路由不同，会在发布 actor 前写入精确过渡事实。路由指纹不含可重建的凭据，见 [session-timeline](../../openspec/specs/session-timeline/spec.md#requirement-cold-model-route-changes-continue-durable-selection)。

会话重命名从查找到标题提交都持有同一生命周期锁，与加载、删除串行；持锁期间直接检查在线 actor，避免等待排在锁后的加载，见 [client-surfaces](../../openspec/specs/client-surfaces/spec.md#requirement-session-rename-participates-in-lifecycle-serialization)。

会话 TTL 清理在配置无效或截止日期无法表示时停止，不把错误配置折算为更短保留期；未配置仍为 30 天，见 [client-surfaces](../../openspec/specs/client-surfaces/spec.md#requirement-session-cleanup-ttl-cannot-wrap-or-overflow)。

TTL 清理在取得写入租约后重新读取并校验同一会话的活动时间，无法确认当前过期状态时保留会话；见 [client-surfaces](../../openspec/specs/client-surfaces/spec.md#requirement-session-cleanup-rechecks-activity-under-writer-ownership)。
