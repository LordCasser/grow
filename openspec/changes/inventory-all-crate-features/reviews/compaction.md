# compaction 逐包核查

包路径：`crates/common/compaction`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；已执行 cargo test --locked -p compaction：56 项通过，无 doctest；14 个 Rust 文件及嵌入提示模板均已读取。

已额外完整读取并登记编译时嵌入的 summary_prompt.txt 模板，其内容作为被审阅代码数据处理。

## 模块与开关

- `crates/common/compaction/Cargo.toml`
- `crates/common/compaction/src/code_compaction/compact.rs`
- `crates/common/compaction/src/code_compaction/config.rs`
- `crates/common/compaction/src/code_compaction/failure.rs`
- `crates/common/compaction/src/code_compaction/mod.rs`
- `crates/common/compaction/src/code_compaction/observer.rs`
- `crates/common/compaction/src/code_compaction/prompt.rs`
- `crates/common/compaction/src/code_compaction/sample.rs`
- `crates/common/compaction/src/code_compaction/summary.rs`
- `crates/common/compaction/src/lib.rs`
- `crates/common/compaction/src/prompt.rs`
- `crates/common/compaction/src/prune.rs`
- `crates/common/compaction/src/reminder.rs`
- `crates/common/compaction/src/sampler.rs`
- `crates/common/compaction/src/token.rs`

- `crates/common/compaction/src/code_compaction/templates/summary_prompt.txt`

Cargo feature：`{}`。

## 功能与规范映射

- [Pure tool result text pruning](../specs/context-compaction/spec.md#requirement-pure-tool-result-text-pruning)：prune_tool_result_content SHALL 将 token 预算饱和乘 4 转为字节上限，已满足预算则返回 None；超限时给头部一半、尾部四分之一，剩余给 marker，各切点保持 UTF-8 字符边界。
- [Oldest first pruning plan](../specs/context-compaction/spec.md#requirement-oldest-first-pruning-plan)：plan_tool_result_pruning SHALL 用 host ItemTokenCounter 一次计算各项 token，按升序选择超出单项预算的 tool result，累计保守节省估计直至总量不大于目标；合计和减法使用饱和计算。
- [Range summary host boundary](../specs/context-compaction/spec.md#requirement-range-summary-host-boundary)：generate_summary SHALL 对 host 选择的非空 turns 构造空 system 与摘要 user prompt，按 SummaryConfig 调用 sampler，返回未经清洗的获胜 summary 与 attempts；空 turns 返回 NothingToCompact。
- [Bounded summary retry outcomes](../specs/context-compaction/spec.md#requirement-bounded-summary-retry-outcomes)：sample_summary_with_retries SHALL 至少尝试一次，正常非空且清洗后不少于 500 字符的摘要立即成功；空白或退化摘要按瞬时失败重试，只有确实继续时等待 retry_delay。
- [Compaction error classification rules](../specs/context-compaction/spec.md#requirement-compaction-error-classification-rules)：HTTP 分类 SHALL 将除 408/429 外的 4xx 视为确定性，任何状态的已知 context length 文本也确定性，其余 transient；stream error 额外识别 code 或 message 中的 invalid_request_error。
- [Summary observer emission](../specs/context-compaction/spec.md#requirement-summary-observer-emission)：摘要循环 SHALL 为每个实际 attempt 发出 Success/EmptyResponse/Degenerate/Failure 回调，携带 1 起始序号、原始摘要或错误与是否将重试。generate_summary 最终成功或失败再发对应终结回调。
- [Canonical summary prompt content](../specs/context-compaction/spec.md#requirement-canonical-summary-prompt-content)：build_summary_prompt SHALL 加载仓库模板并替换 user_context_section；Some context 原样插入上下文段，None 不生成该段。模板要求单一 summary 块、九个编号章节和继续工作的关键信息。
- [Summary cleanup and continuation carrier](../specs/context-compaction/spec.md#requirement-summary-cleanup-and-continuation-carrier)：format_compact_summary SHALL 移除位于 summary 前或紧随开头的 analysis 草稿块，将有效外层 summary 转为 Summary 标题，保留外部文本，并压缩连续三个以上换行及首尾空白。
- [Post compaction active state reminders](../specs/context-compaction/spec.md#requirement-post-compaction-active-state-reminders)：提醒格式化 SHALL 按后台任务、TODO、子 Agent 顺序组合借用的 host 状态，跳过空段；TODO 只展开 Pending/InProgress，Completed/Cancelled 合计放尾注，无行动项时省略整个 TODO 段。

## 边界

- 截短 marker，不侵占头尾份额，结果字节数不超预算；marker 较短时不重新分配空余预算。
- 返回 Some 空字符串；空文本返回 None。
- 不裁剪非工具结果和预算内项，达到目标后不再增加候选。
- 返回空计划；即使返回非空计划也不保证可达到目标，不执行实际内容替换或持久化。
- 总尝试数 3、重试间隔 3 秒、每次传给 sampler 的 timeout 120 秒；自动压缩阈值共享常量为 80%，触发、选区、缩小输入、持久化与提交由 host 负责。
- timeout 参数传给实现，本循环不额外套 tokio timeout；LlmCompactionOutput 只保存 response 字符串，不能由注释推导出 thinking 输出。
- 立即返回 Failure 并标记 deterministic，超限另外标记 context_overflow，不重复相同输入。
- 前者返回 Empty，后者返回 Failure，结果携带尝试数；Timeout/Transient/EmptyResponse 错误默认非确定性，但文本超限规则仍可覆盖。
- 匹配 ASCII 小写化后的 too long for this model、prompt is too long、maximum prompt length、maximum context length、context_length_exceeded，或同时有 current message 与 exceeds budget；普通附件或索引预算错误不自动认定上下文超限。
- 默认 transient；invalid_request_error 标记匹配区分大小写，数值 408/429 同样可重试。
- 成功报告原始摘要字符数、次数和耗时；失败报告次数；空 turns 提前返回，不产生 attempt 或终结回调。observer 默认方法和 () 实现不执行副作用。
- 包含请求意图、技术概念、文件代码、错误修复、问题处理、用户消息、待办、当前工作与下一步，要求不输出独立 analysis 和不调用工具；这些是发送给模型的提示，不是结构化输出校验器。
- 不把正文中 analysis 当作开头草稿删除；剩余 analysis/summary/summary_request 开闭标签在小于号后插入零宽空格，避免保留原控制标签。
- 前置 analysis 丢至下一 summary 或末尾；无有效 summary 配对不强制截正文，剩余标签中和。内层非编号草稿可按最后 analysis close 去掉前缀，编号章节跳过该剥离。
- 前者清洗后加继续会话前言，后者仅原样包 user_query；退化判定按清洗结果 Unicode 字符数少于 500，不按 UTF-8 字节数。
- 仅省略子 Agent 段；已知工具名、任务 ID、描述和状态直接插值，不重新查询存活状态或校验名称。
- 忽略纯空白，非空段用空行连接并包 system-reminder；附加 reminder=None/空白时 summary 不变，其他情况空行追加。
- 只有已完成 TODO 时 is_empty=true；存在子 Agent 时 is_empty=false，即使工具名缺失使渲染结果为空。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。
