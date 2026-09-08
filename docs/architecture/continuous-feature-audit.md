# Grow 持续功能审计

本轮从 2026-09-07 工作区开始。既有未提交修改属于此前修复，保留；`project-review-2026-09-07.md` 和 `trajectory-review-2026-09-07.md` 已记录的修复不重复计数。

每次沿配置、入口、执行、持久化和恢复检查一个功能。发现问题先明确触发条件，再建立回归并修复；无法证明收益的架构调整单独记录。删除候选见仓库根目录 `tmp-feature-removal-candidates.md`，未经用户确认不删除。

## 功能队列

| 功能 | 入口与当前状态 | 审计进度 |
| --- | --- | --- |
| 排队提示合并 | `[ui].combine_queued_prompts` 默认关闭；Pager 设置可开启；Pager 本地 drain 与 Shell promote 共用 `prompt-queue` 规则 | C001–C003 已修复；真实多客户端交互与 Pager 失败重试尚未完整验证 |
| 文件操作锁组件 | `editor_infra` 公共导出，未发现生产调用者 | 列为 R1 删除候选 |
| Goal 自动续跑 | TUI slash → Shell admission → Timeline | C004 计时、C005 迟到未知用量持久化已修复；失败回滚故障窗口待继续 |
| Workflow | Definition / Run / 恢复 | 待深入；已有债务按 ROADMAP 独立处理 |
| 模型与 Provider 路由 | 配置 → sampler → 会话请求 | 待深入 |
| MCP / Skills / Plugins / Hooks | 配置与扩展加载 → 工具运行 | 待深入；插件安装已修复项不重复 |
| Memory | 初始化 / watcher / embedding / search-get | 先前修复已记录，后续查剩余边界 |
| TUI / headless / ACP 会话恢复 | 客户端状态与 Shell / Timeline | 待深入 |

此表是起始队列，不是全仓功能覆盖声明。后续从实际注册表、Cargo feature 和配置字段补齐未启用路径。

## C001 · 合并队列丢失结构化输出约束

状态：已修复，定向回归通过。

`SessionActor::combine_gate` 只判断文本、图片、来源和命令，没有排除携带 `json_schema` 的请求。`combine_front_pending_inputs` 删除 follower 后只合并文本与 input IDs，启动路径 `maybe_start_running_task` 又只读取队首的 `json_schema`。因此普通请求后跟结构化输出请求时，后者的 schema 消失；结构化输出请求位于队首时，其他请求会被纳入它的输出约束。

修复边界：有 schema 的请求独立执行，在现有 Shell gate 中排除；无需增加配置、协议字段或第二套队列。回归覆盖 schema 位于队首、中间和末尾，断言请求仍独立存在、schema 与正文保留，前面的普通提示仍可合并。

生产入口已核对：`agent/mvp_agent/acp_agent.rs` 从 ACP `_meta.outputSchema` 读取 JSON 对象，随 `QueuePrompt` 传入 actor；执行端 `actor/turn/mod.rs` 用 schema 创建结构化输出 validator。因此它不是仅测试可构造的字段。

验证：新增回归在修复前失败（schema 位于队首时队列实际只剩 1 项，预期 3 项）；修复后以下命令 3 passed / 0 failed，覆盖本项、普通合并保留 durable input IDs，以及 Timeline 输入数量上限。`rustfmt --check` 和 `git diff --check` 通过。没有运行真实模型或完整 ACP 会话；链接器报告既有体量相关 `__eh_frame` 警告，不影响测试通过。

```sh
CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline \
  -p shell --lib --quiet combining_prompts -- --test-threads=2
```

下一检查点：已合并消息再次排队的显示元数据、编辑 hold 与恢复。

## C002 · 合并队列覆盖后续请求的 verbatim 语义

状态：已修复，排队 admission 测试组通过。

ACP 入口从 `_meta.verbatim` 读取原样输入模式并存入 `InputItem`。`prompt_parser::render_message` 据此决定是否包裹 `<user_query>`；`actor/turn/admission.rs` 据此决定是否执行大提示截断。合并只留下队首的 `verbatim`，因此 `[普通, 原样]` 会让原样请求变成可截断文本，反向顺序又会让普通请求跳过包裹与截断。

修复边界：只合并与队首 `verbatim` 相同的连续前缀，在首次模式变化时停止。相同模式仍可合并，包括全部为原样输入的队列；schema、图片和编辑 hold 继续使用原有判定，不添加新协议字段。测试覆盖两种交错方向、两个同模式前缀方向和全部同模式的两种队列，并断言合并正文及剩余请求的顺序、模式、正文。

新增回归在修复前失败：`[false, true, false]` 队列实际被合并为 1 项，预期保留 3 项。修复后整个 `follow_up_admission_tests` 组 **10 passed / 0 failed**，包含 C001、C002、持久化输入身份、合并数量上限和 Hook admission 测试。`rustfmt --check`、`git diff --check` 通过。验证未调用真实模型，也未模拟完整 ACP 网络会话；解析与截断影响由生产代码复核确认。

```sh
CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline \
  -p shell --lib --quiet session::actor::prompt_queue::follow_up_admission_tests \
  -- --test-threads=2
```

## C003 · 客户端合并消息的分段在 Shell 入队和再次合并时丢失

状态：已修复，跨入队与恢复回归通过。

Pager 的本地 drain 将原始消息写入文本块 `_meta.combinedDisplayTexts`，供其他客户端及回放显示多个气泡。Shell `queue_input` 却把 `QueueEntryMeta.combined_texts` 固定为空，因此排队广播和运行广播丢失分段；后续再与其他排队请求合并时，`append_text_to_prompt` 也只追加整段 follower 正文，并用扁平化后的错误分段覆盖回放元数据。

修复沿用已有字段：入队时读取客户端分段并随 `StoredQueueEntry` 持久化，再次合并时保留队首和 follower 各自的原始分段。模型正文继续按现有规则连接，不改变请求身份与 admission 生命周期。验证使用实际 actor 入队、磁盘 payload 恢复、queue wire、running display 和最终 ACP 文本块元数据，输入为两个已经合并的中文消息对。

修复前新增回归在入队广播断言失败：实际 `None`，预期 `["一", "二"]`。修复后 `session::actor::prompt_queue::follow_up_admission_tests` **11 passed / 0 failed**，覆盖两对消息入队、清空内存 FIFO 后从已持久化 payload 恢复、再次合并为四个分段、正文顺序及回放元数据。命令同 C002。`rustfmt --check`、`git diff --check` 通过；未运行真实多客户端 TUI，不能将元数据断言声称为完整视觉验收。

分段读取沿用 Pager 回放对非空字符串数组的处理约定，不更改协议。已有队列文字编辑会清除分段并重建文本块，继续遵循“显式重写是一条新正文”的现有行为。

下一功能从 Goal 的状态转换、预算记账与 idle admission 开始。已定位 `session/goal_tracker.rs`、`actor/goal.rs` 与 `actor/goal_support.rs`，架构约束以 `goal-continuation.md` 为起点再逐条对照代码；尚未声称审计完成。

## C004 · 查询已停止的 Goal 会重新启动耗时计量

状态：已修复，GoalTracker 相关回归通过。

`GoalTracker::account_elapsed` 使用 `active_since.replace(Instant::now())`，然后才判断原值是否存在。当 Goal 已暂停、阻塞、预算停止或完成时，原值为 `None`，函数虽然立即返回，却已写入新的起始时间。`elapsed_ms()` 会开始增长；下一次结算又把停止期间的时间写回持久状态。

生产入口：`BuiltinAction::GoalStatus` 每次执行 `/goal status` 都调用此函数；`checkpoint_goal_before_shutdown` 也对保留的停止状态调用它。所以这不是只存在于未使用辅助 API 的问题。修复边界是“没有活动计时器时结算无副作用”，继续沿用当前计时状态，不引入后台时钟或新字段。

新增回归从四种停止状态的合法快照恢复，再反复模拟查询、等待、结算，要求实时与持久化 elapsed 均固定不变。预算扣除与生命周期状态不在本项修改范围。

修复前实测暂停状态从 `123 ms` 增长至 `135 ms`，回归失败；修复后只在原有活动计时器存在时更新起点，四种停止状态的计时均保持不变。以下命令 **26 passed / 0 failed**，包含新增停止状态回归及现有活动计时结算、生命周期、预算和阻塞计数测试。`rustfmt --check`、`git diff --check` 通过。生产 slash 与 shutdown 入口已静态核对，没有运行真实 TUI 状态查询。

```sh
CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline \
  -p shell --lib --quiet goal_tracker -- --test-threads=2
```

下一步重点核对预算结算的持久化失败回滚与异步 owner 结算；当前 `account_captured_goal_usage` 在持久化失败时恢复旧快照，`start_goal_internal_turn` 在等待后重验目标 id、definition revision、foreground 与 pending controls。已读到这些屏障，不把初步读码当成故障注入验收。

## C005 · 已暂停 Goal 的迟到未知用量被结算但未持久化

状态：已修复，真实 actor 结算与恢复回归通过。

`apply_captured_goal_usage_incomplete_outcome` 先标记内存中的 `usage_incomplete`，随后在“有预算、没有 active Step、状态已经 Paused”时直接返回 `Stopped`。该分支没有提交 Control；调用方却将 attempt 视为结算成功并从共享窗口移除。重载旧 Control 后，未知用量的证据消失，原本需要移除预算才能恢复的 Goal 又可能被当成精确预算。

回归先在 Active Goal 注册真实 attempt，再通过现有暂停事务停止 Goal，然后结算迟到的未知用量。验收读取最新 Timeline Control 并恢复 GoalTracker，要求未知用量标记仍存在、有预算时 restart 被拒绝。

修复边界：区分“本次刚发现未知用量”和“已记录该证据”；已经暂停不代表可以跳过新证据的持久化。保留原有 Stopped outcome 与 attempt 结算顺序，不建立第二份用量账本。

修复前新增回归失败于最新 Control 的 `usage_incomplete` 断言，此时 attempt 已被移除。修复后，读取最新 Timeline Control 恢复的 Goal 仍带未知用量标记，预算未移除时无法 restart。以下测试组 **16 passed / 0 failed**，包含现有预算 Step 屏障、无预算未知用量、重复结算窗口和 root/child shutdown 测试。`rustfmt --check`、`git diff --check` 通过。没有调用真实 Provider；测试主动交错暂停与结算以固定迟到条件。

```sh
CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline \
  -p shell --lib --quiet session::actor::goal_support::tests -- --test-threads=2
```

后续故障注入可复用已有测试的 `ChatStateHandle::noop()` 替换方式，核对结算失败时 attempt 保留、计费回滚及重试。后台 compaction 取消属于另一条可失败边界，需结合 writer epoch 的终止规则验证，当前没有把读码疑点计作已确认 bug。

## 未默认启用路径的初步清单

已检查 CLI、Pager、Shell、Tools、Agent 的 Cargo feature 声明。编译特性和产品功能不是一一对应，后续按实际使用点审计，不能把空 feature 直接当作可删除功能。

| 路径 | 当前证据 | 后续动作 |
| --- | --- | --- |
| `shell/unstable` | 空 feature，默认关闭，未发现 cfg 使用点或脚本启用点 | 已列为 R2 低优先级删除候选，等待确认 |
| `shell/test-support`、`pager/test-support` | 明确服务测试与 bench，默认生产构建不启用 | 不按闲置产品功能删除 |
| `shell/dhat-heap`、`tools/dhat-heap` | 可选堆分析依赖 | 核对启用后采集入口 |
| `cli/distro-pm` | 下游包管理器构建关闭自更新 | 核对更新入口是否一致遵守 |
| `pager/jemalloc` | 声明明确说明是空转接，allocator 位于 CLI | 不因空实现判定冗余 |
| `cli/release-dist` → `pager/release-dist` | 发布构建特性 | 核对 cfg 行为与发布脚本 |

## 独立债务与验证限制

- 旧审计的通用编辑提交步骤、投影性能优化继续保留在原报告，不混入队列修复。
- ROADMAP 中明确搁置的功能不因本次审计自动启用。
- Atlas 本轮局部 symbol 查询返回了与当前源码不一致的行号和部分同名方法解析；调用判断以实际源码复核为准，不将其空 callers 当作全仓无引用的证明。
