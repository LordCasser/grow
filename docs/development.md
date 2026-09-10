# 开发流程

项目使用 OpenSpec SDD。先把本次要改变的行为写清楚，再实现和验证，最后归档成为当前规范。纯文档修正也保留最小变更记录，但不增加无意义需求。

采样断流排查从 attempt evidence 的 `attempt_number`、`stream_end`、
`output_delivery`、`output_observed` 和 `recovery_stop` 决定开始，再核对
Timeline 的 `sampling_usage/attempt_settled`。`[DONE]` 或 body EOF 不能代替
协议完成证据；没有对应会话的原始尾帧时，不能仅凭错误文字归因于代理。
当前 `GROW_MAX_RETRIES` / `max_retries` 沿用字段名，表示同一未接纳模型步骤的
总 attempt 上限，包含初次调用；0 仅允许初次调用。session 默认上限为 5，
独立 sampler 默认值为 15。共享期限取首次 idle timeout × 总上限，后续修复
不会延长；精确预算或不可撤销输出可能更早关闭恢复。
实现导航见 [采样恢复边界](architecture/session-robustness-repair.md)，
行为以 [model-sampling](../openspec/specs/model-sampling/spec.md) 为准。

## 环境

Rust toolchain 由 `rust-toolchain.toml` 声明，构建仍使用 Cargo。OpenSpec 是开发工具，不进入 Cargo 运行时依赖。

```sh
npm install -g @fission-ai/openspec@1.11.0
openspec --version
openspec list
openspec list --specs
```

本仓库通过根 AGENTS.md 和 CLI 接入，未提交个人 AI 工具配置。下面的 CLI 命令不要求 `/opsx:*` 已安装；需要工具专属交互时可自行运行 `openspec init --tools <tool>`，不要覆盖项目规则。

## 一次行为变更

1. 读取相关 spec、实现、调用方和测试。若代码与 spec 不一致，在 proposal 说明发现与处理方向。
2. 建立 change，按 CLI 返回的上下文逐项写 artifact。

```sh
openspec new change fix-example --schema spec-driven
openspec instructions proposal --change fix-example
openspec instructions specs --change fix-example
openspec instructions design --change fix-example
openspec instructions tasks --change fix-example
openspec status --change fix-example
openspec validate fix-example --strict --no-interactive
openspec instructions apply --change fix-example
```

`fix-example` 是占位名称。每个 change 包含 `proposal.md`、`design.md`、`tasks.md` 和 `specs/<capability>/spec.md` delta。修改已有要求用 `## MODIFIED Requirements` 并保留该要求的完整正文和全部仍成立的场景；新增用 `## ADDED Requirements`，删除写原因及迁移影响。

```markdown
## ADDED Requirements

### Requirement: Example behavior
系统 SHALL 在明确条件下产生可验证结果。

#### Scenario: 明确边界
- **WHEN** 触发具体条件
- **THEN** 观察到具体结果
```

3. 按 tasks 实现，更新对应 docs 解释，执行与影响范围匹配的检查；验证记录放在 change 的 `verification.md`。需要验证正常、错误或恢复路径时使用有意义的测试。未执行、失败、外部条件缺失必须写清楚。
4. 确认场景落实后勾选 tasks，归档同步主规范，再次校验。

```sh
openspec validate --all --strict --no-interactive
openspec archive fix-example --yes
openspec validate --all --strict --no-interactive
openspec validate --archived --no-interactive
```

归档是维护规范的本地操作，不代表 Git 已提交、合并或发布。CLI 的 artifact 状态只检查文件是否存在；格式通过也不能证明行为正确。

## 不改行为的最小变更

先创建 change，在生成的 `.openspec.yaml` 中保留 schema/created 并加入 `skip_specs: true`。proposal 写明不改行为的原因，design 简述影响，tasks 只列必要动作和验证。不创建空 delta，也不为工具维护虚构产品能力。完成后使用 `openspec archive <change> --skip-specs --yes`，再运行上述全量与归档校验。

## Rust 验证入口

按受影响 crate 缩小检查范围；下列命令是项目现有 CI/README 的入口，不意味着每次纯文档修改都运行全部测试。

```sh
cargo check --locked -p cli
cargo test --locked --lib -p chat-state -p sampling-types -p sampler -p memory -p workflow -p shell -p pager -p pager-minimal -- --test-threads=4
cargo build --locked -p cli --bin grow
```

原生 debug CLI 的会话线程为未优化 async 临时状态预留 32 MiB 栈；release 仍使用 8 MiB。该线程显式指定栈大小，不受测试运行器的 `RUST_MIN_STACK` 控制，不为调试栈开销改动生产调用链。

核心、跨平台协调及 Windows 存储回归统一设置 `RUST_MIN_STACK=16777216`，为调试测试夹具提供足够的测试线程栈。

跨会话协调与 Windows 存储还应检查对应 `.github/workflows/` 的平台回归。OpenSpec CI 只做文档格式与归档完成状态检查，语义由场景、源码、测试和 review 共同核对。

用量状态栏由 `ChatStateEvent::SessionUsageUpdated` 投影到账本变化时的 transient `SessionInfoUpdate.meta["grow/sessionUsage"]`，复用 `PromptUsage`，不增加周期查询、Timeline 消息或模型输入。新建/重新连接时的状态 advertisement 补发当前账本；Pager 按累计值替换、丢弃同窗口倒退及历史 replay，用 reload 清空旧窗口。计费窗口和点击行为见 [会话用量契约](../openspec/specs/client-surfaces/spec.md#requirement-ordinary-agent-status-shows-session-usage)。

## 债务与历史

无关审计发现登记 [OpenSpec backlog](../openspec/backlog.md)，明确条件、影响、证据和未来验收；等待单独启动。既有过程文档见 [历史登记](../openspec/baseline.md#历史资料)，不延续其中的任务清单。

托管配置语法检查器的退出与清理契约见 [configuration-rules](../openspec/specs/configuration-rules/spec.md#requirement-managed-syntax-validator-exit-cleanup)；实现位于 `config/src/managed_text/validator.rs`。

托管配置的回滚冲突与恢复材料边界见 [configuration-rules](../openspec/specs/configuration-rules/spec.md#requirement-managed-config-rollback-preserves-observed-conflicts)；文件锁与检查不能替代非协作编辑器之间的原子比较交换。

托管配置实际读取量限制见 [configuration-rules](../openspec/specs/configuration-rules/spec.md#requirement-managed-config-reads-obey-byte-budgets)，元数据预检不代替读取预算。

托管配置源快照的同句柄约束见 [configuration-rules](../openspec/specs/configuration-rules/spec.md#requirement-managed-source-fields-share-a-file-handle)，它不等于对原地并发写入的原子快照。

托管配置规划必须保持源可读性，见 [configuration-rules](../openspec/specs/configuration-rules/spec.md#requirement-managed-plans-preserve-source-readability)；最终大小包含原文、标记和条目正文。

托管配置批量渲染复用原文解析计算条目状态和替换范围，保留完整输出校验。性能探针和前后数据见 [batch-managed-config-render](../openspec/changes/archive/2026-09-08-batch-managed-config-render/verification.md)。

占位图片加载与恢复共用 `client-support/src/placeholder_images.rs` 的有界读取入口，契约见 [client-surfaces](../openspec/specs/client-surfaces/spec.md#requirement-placeholder-image-caps-bound-actual-reads)。检查文件大小不能代替限制实际读取量。

占位图片恢复还会在读取前传入本次恢复的剩余总预算，见 [恢复预算契约](../openspec/specs/client-surfaces/spec.md#requirement-orphan-image-reads-honor-remaining-recovery-budget)。总预算停止与单图超限跳过是不同的处理分支。

图片附件URI使用 `client-support::placeholder_images::file_uri_from_path`，不能直接拼接路径字符串。生成与解析边界见 [图片URI契约](../openspec/specs/client-surfaces/spec.md#requirement-image-file-uris-preserve-literal-path-bytes)；占位符正文仍使用原始文件路径。

Dashboard 的路径粘贴共用 `insert_dropped_paths`，保留图片和普通文件的顺序；不要在插入前使用图片专用过滤器。见 [混合路径契约](../openspec/specs/client-surfaces/spec.md#requirement-dashboard-preserves-mixed-drop-paths)。问题模式和异步目标有效性由调用入口检查。

异步文件 URL 和原始剪贴板文本是两个独立来源。分类未命中时，通过 `ClipboardPasteSource::file_url_text_on_miss` 判断是否需要保留 URL 文本；仅在成功探测且没有原始非空文本时使用。见 [异步 URL 回退契约](../openspec/specs/client-surfaces/spec.md#requirement-deferred-file-urls-survive-classification-miss)。

Kitty 非 PNG 转换在调用 sips 或 Rust 解码前检查源像素预算，见 [转换像素契约](../openspec/specs/client-surfaces/spec.md#requirement-kitty-image-conversion-bounds-source-pixels)。编码数据大小、源像素数、转换进程生命周期和终端直接解码是不同的资源边界。

sips 源文件和结果文件由单个私有临时目录持有，错误出口也通过目录所有权回收，见 [转换临时文件契约](../openspec/specs/client-surfaces/spec.md#requirement-sips-conversion-owns-private-temporary-files)。不要恢复为共享临时根目录中的手工命名文件和分散清理。

sips 通过 `run_sips_command` 在独立进程组中运行，执行期限为10秒，返回前处理进程组回收；见 [转换执行契约](../openspec/specs/client-surfaces/spec.md#requirement-sips-converter-has-an-owned-execution-deadline)。该期限不涵盖文件I/O或不可中断的内核等待。

sips 结果用同句柄检查及有界读取限制为100MB，见 [结果读取契约](../openspec/specs/client-surfaces/spec.md#requirement-sips-output-reads-have-an-encoded-budget)。这不限制转换器写入磁盘前的产物大小，也不覆盖 Rust 回退编码器。

sips 进程错误保留启动、进程组登记、等待或清理阶段及原始错误文本，见 [进程诊断契约](../openspec/specs/client-surfaces/spec.md#requirement-sips-process-failures-identify-their-stage)。不能仅凭 EPERM 推断是清理失败。

图片查看器的真实 Enter 入口只捕获内存/文件来源与终端协议，读取和转换由 `LoadImageViewer` 任务完成；每次打开使用独立 owner，见 [查看器加载契约](../openspec/specs/client-surfaces/spec.md#requirement-prompt-image-viewer-loading-is-deferred)。测试必须覆盖真实入口，不能仅手工构造 loading 状态来证明生产接线。

查看器后台加载在内存复制或文件读取时执行50MB源字节预算，见 [查看器来源预算](../openspec/specs/client-surfaces/spec.md#requirement-background-image-viewer-source-bytes-are-bounded)。它与拖入图片共用有界文件读取，不代替并发任务或转换内存预算。

Slash MRU 的进程内写线程不能协调其他 Grow 进程；每次快照使用目标目录中的独占临时文件再替换，见 [MRU 写入契约](../openspec/specs/client-surfaces/spec.md#requirement-slash-mru-writes-own-unique-temporary-files)。完整快照仍采用最后写入者覆盖，不等于跨进程历史合并。

Slash MRU 首次同步加载只接受普通文件，编码输入上限为1 MiB；拒绝读取时禁用本会话持久化以保护源文件，见 [MRU 加载契约](../openspec/specs/client-surfaces/spec.md#requirement-slash-mru-loading-bounds-encoded-input)。这不提供慢文件系统的读取超时。

Slash MRU 后台仅保留一个待写最新完整快照，写入在锁外执行；线程不可用时保留dirty重试，不回退到UI同步写盘，见 [MRU 调度契约](../openspec/specs/client-surfaces/spec.md#requirement-slash-mru-background-writes-coalesce-pending-snapshots)。进程退出仍不保证flush。

历史搜索在提交时合并待处理items/query，用容量1通知唤醒匹配线程；提交和关闭不等待队列容量，见 [历史搜索调度契约](../openspec/specs/client-surfaces/spec.md#requirement-history-search-submission-does-not-wait-for-worker-capacity)。正在执行的匹配不会被强制中断。

历史搜索结果回传提交时的请求编号；UI仅接受当前请求，并在新查询/刷新时清除旧的可选结果，见 [历史结果契约](../openspec/specs/client-surfaces/spec.md#requirement-history-search-accepts-only-current-request-results)。

本地草稿恢复检查打开句柄为普通文件，并在解析前检查实际读取是否超过256KiB；超限沿用隔离策略，见 [草稿读取契约](../openspec/specs/client-surfaces/spec.md#requirement-local-draft-recovery-bounds-source-reads)。Unix特殊文件打开不等待FIFO写入者。

本地草稿的composer与staged_prompt必须互斥；共享校验在空记录删除前执行，异常磁盘记录隔离而非选择其中一份恢复，见 [草稿互斥契约](../openspec/specs/client-surfaces/spec.md#requirement-local-draft-records-contain-at-most-one-prompt-source)。

Agent最后一个草稿所有者关闭后，检查点成功才释放干净运行时缓存；失败保留最新内容和重试期限，重开优先恢复它，见 [关闭草稿生命周期](../openspec/specs/client-surfaces/spec.md#requirement-closed-agent-drafts-release-only-recoverable-runtime-state)。磁盘草稿不会因释放缓存而删除。

本地草稿无效化失败保留逐键删除期限，关闭和会话绑定不会丢失意图；重开抑制旧内容恢复，新有效草稿取消该键旧删除，见 [草稿无效化契约](../openspec/specs/client-surfaces/spec.md#requirement-local-draft-invalidation-retries-without-restoring-stale-content)。意图只在内存中，持续I/O失败后进程退出不保证跨重启清理。

/export路径补全最多消费1000个目录迭代结果，隐藏项及错误也计数，最终最多返回100个建议，见 [导出补全预算](../openspec/specs/client-surfaces/spec.md#requirement-export-path-completion-bounds-directory-enumeration)。单次文件系统调用延迟不受此数量预算保证。

滚动日志仅接受普通文件目标，打开句柄检查后才截断；Unix FIFO不等待reader，失败后使用已有禁用状态，见 [滚动日志目标契约](../openspec/specs/client-surfaces/spec.md#requirement-scroll-recorder-rejects-special-file-targets)。普通显式文件覆盖和符号链接行为保留。

单个滚动日志记录器最多接受64MiB（含换行），按完整行停止、flush并报告关闭，不自动轮转或删除旧文件，见 [滚动日志字节预算](../openspec/specs/client-surfaces/spec.md#requirement-scroll-recording-bounds-each-capture-by-complete-lines)。末尾手势可能尚未finalize，不能把停止当成完整实验结束。

输入诊断由实际处理按键的AgentView记录，委派子视图不再消费父textarea delta；导出解析子视图及Dashboard附着目标，见 [诊断目标契约](../openspec/specs/client-surfaces/spec.md#requirement-input-diagnostic-dumps-describe-the-input-owner)。Dashboard的Esc仍用于关闭popup，未增加快捷键入口。

Pager统一日志使用初始化时保存的ACP发送端和Tokio runtime，可从普通线程发起批次转发；初始化前flush保留缓冲，重复初始化不增加timer。见[日志转发归属契约](../openspec/specs/client-surfaces/spec.md#requirement-pager-log-dispatch-preserves-initialized-ownership)。flush_blocking只等待本次取出的批次，不代表此前所有后台批次已经完成。

退出时当前日志批次的确认等待最多2秒，避免诊断发送阻止终端恢复；超时只结束本地等待，已入队通知仍可能被处理，见[flush等待契约](../openspec/specs/client-surfaces/spec.md#requirement-pager-log-flush-has-a-bounded-delivery-wait)。

Recap扩展只在会话命令成功入队后确认接纳；命令接收端已关闭时返回ACP错误，手动请求沿既有错误路径清理等待提示，见[Recap接纳契约](../openspec/specs/client-surfaces/spec.md#requirement-recap-admission-reflects-command-enqueue)。配置默认开启，可由远端设置、配置或环境变量关闭。

Pager读取recap扩展响应中的接纳结果；disabled、拒绝或无效响应会结束对应会话的手动等待提示，自动请求失败保持安静，见[Recap响应消费契约](../openspec/specs/client-surfaces/spec.md#requirement-pager-consumes-recap-admission-outcomes)。单次禁用响应不会永久改写连接能力。

回放中的recap只恢复历史展示，不结束当前手动recap等待，也不消耗本次离开期间的自动资格，见[Recap回放契约](../openspec/specs/client-surfaces/spec.md#requirement-replayed-recaps-do-not-settle-current-feedback)。

自动recap的展示记录和重试退避按SessionId隔离；轮询与返回焦点均查询当前根会话，新的离开周期清空所有会话记录，见[自动recap归属契约](../openspec/specs/client-surfaces/spec.md#requirement-automatic-recap-bookkeeping-is-session-scoped)。

公告隐藏状态通过同目录独占临时文件写入、sync后原子替换，失败沿pager持久化结果传递，不再恒报成功，见[公告提交契约](../openspec/specs/client-surfaces/spec.md#requirement-hidden-announcement-state-commits-complete-snapshots)。这不提供跨进程合并或异步请求顺序保证。

共享atomic state writer仅在成功独占创建临时文件后执行失败清理，名称碰撞不会删除已有临时文件，见[临时文件归属契约](../openspec/specs/configuration-rules/spec.md#requirement-atomic-state-writers-preserve-unowned-temporary-paths)。

公告隐藏状态读写共用1 MiB编码字节上限；读取只接受普通文件，超限或非法输入按全部可见处理且不改源文件，见[公告IO边界契约](../openspec/specs/client-surfaces/spec.md#requirement-hidden-announcement-state-has-bounded-io-admission)。Unix FIFO非阻塞拒绝不等于普通慢盘读取有总时限。

公告偏好在单个AppView内最多一个写入任务；隐藏、显示和更新清理在等待期间合并为最新集合，完成后再提交，见[公告写入顺序契约](../openspec/specs/client-surfaces/spec.md#requirement-announcement-preference-writes-follow-local-change-order)。跨进程合并和退出flush不在此保证内。

认证 helper 的失败诊断位于 `shell/src/auth/token_output.rs` 和 `auth_provider.rs`。排查时使用退出状态、JSON 类别/行列及捕获字节数；不要把 helper 原始输出重新拼进日志。见 [凭据输出诊断契约](../openspec/specs/configuration-rules/spec.md#requirement-credential-helper-failures-do-not-echo-output-payloads)。

认证 helper 的相对 cwd 以 Grow 进程工作目录为基准解析一次，再用于程序路径和子进程工作目录。见 [helper 路径契约](../openspec/specs/configuration-rules/spec.md#requirement-relative-credential-helper-cwd-resolves-once)。

认证 helper 的提前刷新窗口与发送有效期分别判断；短效 token 在实际过期前可发送，刷新失败会撤下旧值。实现入口为 `auth_provider.rs::cached_token` / `ensure_fresh_token`，见 [短效凭据契约](../openspec/specs/configuration-rules/spec.md#requirement-valid-short-lived-helper-tokens-remain-sendable)。

采样认证排障使用 auth_type、auth_scheme 与认证头 presence 字段；client_post 和 sampling_request 不记录凭据前后缀。见 [采样认证日志契约](../openspec/specs/model-sampling/spec.md#requirement-sampling-authentication-logs-omit-credential-fragments)。401 attribution 回调为独立路径。

`grow trace` 的 CLI 分发直接进入会话快照导出，不要求模型配置能够成功解析。会话缺失和输出失败仍按原路径报告，见 [Trace 配置独立性契约](../openspec/specs/client-surfaces/spec.md#requirement-trace-export-does-not-require-valid-model-configuration)。

已批准的闲置接口清理边界以 [配置规则](../openspec/specs/configuration-rules/spec.md)、[工具协议](../openspec/specs/tool-authorization/spec.md)、[技能运行时](../openspec/specs/extension-runtime/spec.md) 和 [客户端设置持久化](../openspec/specs/client-surfaces/spec.md) 为准。技能列表由 SkillManager 管理；配置整份写入入口仍保留。
