# extension-runtime Specification

## Purpose
定义外部工具与生命周期 Hook 的集成职责。MCP 管理支持的传输和工具调用，Hooks 在执行前形成明确计划并记录跳过原因；这些扩展入口的存在不替代调用方的权限判定。

## Requirements

### Requirement: MCP transport ownership
MCP 层 SHALL 负责 stdio 子进程与 Streamable HTTP 连接、工具调用和生命周期管理。

#### Scenario: 调用外部 MCP 工具
- **WHEN** 已配置 MCP server 通过支持的 transport 建立连接
- **THEN** 工具经 MCP server 层调用并分类处理连接或调用错误。

证据：`crates/codegen/mcp/src/lib.rs` — `servers`。

### Requirement: Explicit hook planning
Hooks SHALL 按事件、匹配条件、启用状态与策略形成 Execute 或 Skip 计划。

#### Scenario: Hook 被禁用
- **WHEN** 命中的 Hook 显式 disabled 或被 policy 禁用
- **THEN** 计划记录对应跳过原因，不执行该 handler。

证据：`crates/codegen/hooks/src/dispatcher.rs` — `plan_dispatch_with_policy`。

### Requirement: Recovery claims are owned before task polling
MCP stdio 重启与 HTTP 恢复 SHALL 在取得去重标记后立即将释放责任交给任务所有的守卫，包括任务首次运行前被丢弃的情况。

#### Scenario: 未运行任务被销毁
- **WHEN** 调度已取得恢复标记但 LocalSet 在任务首次 poll 前销毁
- **THEN** 标记被释放，不执行恢复动作或状态推送，同名标记可再次取得。

### Requirement: Recovery configuration waits are cancellable
MCP stdio 和 HTTP 恢复 SHALL 在调度及重试循环的异步配置探测期间响应取消，不依赖探测先完成。

#### Scenario: 探测期间取消
- **WHEN** 恢复调度或循环等待配置探测且取消 token 被取消
- **THEN** 结束等待、不执行恢复动作、不推送 Disabled 或成功状态，已取得的恢复标记释放。

### Requirement: Dispatcher exit cancels owned recoveries
MCP dispatcher SHALL 在正常退出和任务被强制终止时取消其启动的恢复任务。正常关闭保留等待恢复标记清理的流程。

#### Scenario: Dispatcher 被 abort
- **WHEN** 已调度恢复且 dispatcher 因 fatal 或关闭超时被 abort
- **THEN** 恢复 token 被取消，等待中的恢复不继续发起动作且释放去重标记；abort 本身不承诺同步等待全部任务。

### Requirement: Superseded liveness watchers cannot clear replacements
MCP 旧 liveness watcher SHALL 在共享锁内确认自己未被替换取消后才能清理槽位；已取消的旧 watcher SHALL 不清理新句柄或发送该次关闭事件。

#### Scenario: 状态检查期间替换
- **WHEN** 旧 watcher 等待状态检查时其 handle 被替换，随后旧 watcher 进入退出清理
- **THEN** 新 handle 保留且其 token 未被旧清理取消；新 watcher 仍可自行清理退出。

### Requirement: Concurrent recovery admission is atomic
MCP recover SHALL 在同一状态锁内检查 Ready 并重置为 Pending，使竞争的恢复调用加入同一个后续握手。

#### Scenario: 两个恢复竞争 Ready
- **WHEN** 两个恢复调用在锁竞争下尝试恢复同一个 Ready client
- **THEN** 仅一次 Ready 到 Pending 重置，随后两者共享新握手产生的服务。

### Requirement: Late tool errors do not reset replacement services
MCP 工具可恢复错误 SHALL 绑定失败调用使用的服务身份；当前 Ready 服务已替换时 SHALL 不重置新服务，而使用当前服务执行既有的一次重试。

#### Scenario: 旧调用错误迟于恢复完成
- **WHEN** 旧服务上的工具调用返回可恢复错误，而新服务已经完成握手
- **THEN** 不触发额外握手，仍按最多一次工具重试的策略返回结果。

### Requirement: Late tool timeouts preserve replacement services
MCP 工具超时 SHALL 仅在当前 Ready 仍是该调用使用的服务时重置 transport；新服务或进行中的握手 SHALL 不被旧超时覆盖。超时 SHALL 不自动重放工具调用。

#### Scenario: 旧请求迟到超时
- **WHEN** 旧请求超时时其他恢复已经安装新 Ready 服务
- **THEN** 返回超时错误，保留新服务且不增加握手或工具重试。

### Requirement: Superseded respawns do not publish failure or clean tools
stdio MCP 重启发现其配置已被替换时 SHALL 返回独立失效结果，恢复循环 SHALL 结束旧任务而不推送失败、不继续重试或删除服务器工具。

#### Scenario: 最后一次握手配置失效
- **WHEN** 最后一次重启握手发现配置 generation 或内容已变化
- **THEN** 不产生该次失败/耗尽状态，不执行耗尽工具注销；之前真实失败记录保留。

### Requirement: Liveness watchers do not own idle clients
MCP liveness watcher SHALL 在检查间隔只持有客户端弱引用，不因长期监测延长客户端生命；发现客户端已释放时 SHALL 清理自身槽位并静默退出。

#### Scenario: 最后外部引用释放
- **WHEN** 检查间隔或首次 tick 前最后一个外部客户端强引用释放
- **THEN** watcher 不阻止客户端销毁，后续 tick 清理自身槽位且不产生 TransportClosed。

### Requirement: Respawn completion belongs to the installed client
stdio MCP 重启 SHALL 在最后异步监听器初始化之后、发送工具刷新和返回成功之前，在同一状态锁内确认已安装客户端身份仍为当前 owned client。失配 SHALL 返回 Superseded，且不借用替代客户端身份发送刷新。

#### Scenario: 监听器初始化期间连接替换
- **WHEN** 重启已安装客户端，等待监听器初始化期间同名客户端被移除或替换
- **THEN** 旧任务返回 Superseded，不发出该次工具刷新或重启成功状态。

#### Scenario: 其他服务器配置变化
- **WHEN** 监听器初始化期间只有其他服务器配置变化，当前 owned client 身份保持
- **THEN** 本次重启仍可完成工具刷新并返回成功。

### Requirement: HTTP recovery outcomes retain client identity
HTTP MCP 恢复 SHALL 在传播成功或失败之前，在同一状态锁内核对捕获客户端仍为当前连接及 HTTP 配置仍存在，并核对同步读取的禁用状态。缺失、替换或禁用 SHALL 分类为 Superseded；恢复循环 SHALL 立即结束旧任务，不按名称重试替代客户端。

#### Scenario: 握手结果返回前配置替换
- **WHEN** HTTP 恢复的成功或失败结果返回时捕获客户端已被替换
- **THEN** 结果分类为 Superseded，旧循环不继续恢复替代连接。

#### Scenario: 当前连接真实失败
- **WHEN** 客户端身份和配置仍有效但恢复失败
- **THEN** 继续现有有限退避重试。

### Requirement: Initialization guards cover result commit waits
MCP 初始化 SHALL 保持取消恢复守卫有效直到取得结果写入状态锁；撤销守卫与写入结果之间 SHALL 无异步等待。

#### Scenario: 提交锁等待期间取消
- **WHEN** 可重建连接的握手结果已完成、提交等待状态锁，锁释放后初始化 future 被取消
- **THEN** 取消守卫恢复 Pending，不遗留 Initializing。

### Requirement: Nonrestorable initialization cancellation releases waiters
不可重建的 stdio 初始化被取消时，在状态锁可取得且仍为 Initializing 的情况下，取消守卫 SHALL 将状态置为 Empty 并通知等待者；不可重建 SHALL 不被当作守卫已撤销。

#### Scenario: stdio 握手取消
- **WHEN** stdio 握手等待响应时取消，状态锁可取得
- **THEN** 状态变为 Empty、等待者被通知，后续初始化立即返回无 transport 错误。

### Requirement: Initialization cancellation survives state lock contention
MCP 初始化取消 SHALL 在短时状态锁竞争结束后完成目标状态恢复，不得因 try_lock 失败丢弃清理责任。等待握手、通知或其他异步 IO SHALL 不持有客户端状态锁。

#### Scenario: 其他线程暂持状态锁
- **WHEN** 初始化取消时另一线程暂时持有客户端状态锁，随后释放
- **THEN** 取消恢复完成，状态从 Initializing 转为 Pending 或 Empty，不遗留中间状态。

### Requirement: Command hook output capture is bounded
命令 Hook SHALL 在读取期间限制每路 stdout/stderr 缓存为 64 KiB 加一个截断检测字节，并持续排空超出数据；stdin 写入与两路输出读取 SHALL 并发且受原执行超时约束。

#### Scenario: 输出超过上限
- **WHEN** 子进程输出远超 64 KiB
- **THEN** 保留前缀和截断标记，缓存不随输出总量增长，输出管道仍被排空。

#### Scenario: 输出恰好达到上限
- **WHEN** 单路输出恰好 64 KiB
- **THEN** 完整保留且不添加截断标记。

### Requirement: HTTP hook decision bodies are bounded
阻塞型 HTTP Hook SHALL 在读取期间限制正文为 64 KiB；收到超出上限的分块时 SHALL 立即返回 Failed，不等待 EOF、不解析部分正文。超限 SHALL 由原有 hook 失败策略处理。

#### Scenario: 分块响应超过上限且不结束
- **WHEN** 端点发送超过 64 KiB 正文后保持连接未结束
- **THEN** Hook 返回响应超限失败，不等待请求超时或正文结束。

#### Scenario: 合法边界响应
- **WHEN** 正文为空或恰好 64 KiB
- **THEN** 正常读取并按既有决策解析处理。

### Requirement: HTTP hooks connect to validated addresses
HTTP Hook SHALL 使用 URL 校验阶段取得且全部通过 IP 检查的地址集合建立直连，不在请求时重新解析目标或自动采用系统代理。原 URL 主机名 SHALL 用于 Host 与 TLS 身份校验，跳转仍禁止。

#### Scenario: 校验后域名解析变化
- **WHEN** 地址已校验而域名后续解析变化
- **THEN** 请求仍使用已校验地址集合，不通过后续解析选择新地址。

#### Scenario: IPv6 字面地址
- **WHEN** URL 主机是 IPv6 字面地址
- **THEN** 直接按 IP 分类校验，不执行 DNS 解析。

### Requirement: HTTP hook phases share one timeout budget
HTTP Hook 的 URL 校验、请求和正文异步等待 SHALL 共享一次 timeout_ms 预算，阶段切换不得重置总预算。超时 SHALL 返回 TimedOut 并保留已经取得的 URL/状态码信息。同步代码不提供抢占式超时。

#### Scenario: 校验与正文分别耗时
- **WHEN** URL 校验与正文读取各自短于预算但合计超过预算
- **THEN** 总预算耗尽时返回 TimedOut；若已收到响应头则保留其状态码。

### Requirement: Command hook abnormal exits terminate process groups
命令 Hook 在进程组成功建立的情况下，取消或超时 SHALL 终止整个进程组，不依赖 session scope 是否存在。清理责任 SHALL 由执行 future 持有的守卫承担。

#### Scenario: 取消已派生孙进程的 Hook
- **WHEN** Hook 已派生同组后台进程后执行 future 被取消
- **THEN** 同组后台进程被终止，无 scope 时亦如此。

#### Scenario: 无会话 scope 超时
- **WHEN** 未提供 session scope 且 Hook 超时
- **THEN** 超时返回 TimedOut 并终止已建立的进程组。

### Requirement: Hook allow decisions cannot hide execution failures
Prompt/Tool Hook 的 allow SHALL 仅在命令退出码 0 或 HTTP 2xx 时有效；其他执行失败 SHALL 交给现有 on_failure 策略。命令退出码 2 和显式 deny/block 仍保留拒绝优先语义。

#### Scenario: allow 后命令失败
- **WHEN** 命令输出 allow 后以非 0/2 退出，on_failure 为 block
- **THEN** 记录 Failed 并拒绝操作。

#### Scenario: 非成功 HTTP 允许正文
- **WHEN** HTTP 非 2xx 响应正文为 allow
- **THEN** 返回 Failed，不将正文作为成功允许。

### Requirement: Structured hook output errors are failures
命令与 HTTP Hook 决策解析 SHALL 将对象/数组前缀的 JSON 语法错误以及 JSON 数据/schema 错误分类为 Failed，不当作普通日志成功允许。空输出和普通非 JSON 文本保留既有规则。Prompt/Tool 命令 exit 2 SHALL 不因解析错误或未知决策而失去拒绝优先。

#### Scenario: 错字段或截断决策
- **WHEN** 成功执行输出决策对象但存在未知字段、字段类型错误或截断对象
- **THEN** 记录 Failed 并应用现有失败策略。

#### Scenario: 退出码拒绝与错误正文并存
- **WHEN** Prompt/Tool 命令以 2 退出且正文协议错误
- **THEN** 仍拒绝操作。

#### Scenario: 数组冒充决策对象
- **WHEN** 输出是 JSON 数组，包括空数组
- **THEN** 返回 Failed，不使用 Serde 的位置字段映射生成默认决策。

### Requirement: Hook deduplication tolerates missing display source
Hook 去重 SHALL 在 command_raw/url_raw 缺失时使用对应实际 command/url，不能把不同实际内容折叠为空字符串键。相同内容的 first-wins 行为保持。

#### Scenario: 不同实际命令或 URL 无 raw
- **WHEN** 多个 Hook 的 raw 字段缺失但实际 command 或 URL 不同
- **THEN** 每个不同 Hook 都保留；实际值相同时仅保留首个。

### Requirement: Relative executable hook deduplication includes its base directory
直接执行的相对 Hook 命令去重 SHALL 包含 source_dir，不能合并来自不同目录的同名相对脚本；去重与执行 SHALL 共用 shell 路由判定。shell 命令与绝对路径保持既有跨目录去重规则。

#### Scenario: 多目录同名脚本
- **WHEN** 两个 Hook 命令文本相同且为直接相对路径，但 source_dir 不同
- **THEN** 两个 Hook 均保留。

#### Scenario: 同一 workspace 的 shell 命令
- **WHEN** 同一 shell 命令来自不同 source_dir
- **THEN** 仍按既有 first-wins 去重，不因 source_dir 重复执行。

### Requirement: Invalid hook matchers never execute without match values
Tested 事件恢复时因无效模式产生的 Never 匹配器 SHALL 在事件有无 match_value 时均不匹配；缺失字段的宽容规则 SHALL 不覆盖该状态。有效匹配器的原规则保持。

#### Scenario: 无效匹配器遇到无字段事件
- **WHEN** Tested 事件的 Hook 模式编译失败且事件未提供匹配值
- **THEN** 匹配判断返回 false，执行计划跳过该 Hook。

### Requirement: Hook registries own matcher reconstruction
HookRegistry SHALL 在反序列化及规格追加/去重接纳时从 configured_matcher 重建 matcher，不信任传入缓存。Ignored 事件保留配置原文但清除 matcher，不编译或筛选；Tested 事件合法模式编译，非法模式 Never，缺失模式清除缓存。wire 调用者不必另行重编译。

#### Scenario: 序列化往返
- **WHEN** 带配置模式的 registry 完成 serde 往返
- **THEN** 立即保持相同匹配范围，无须额外调用修复。

#### Scenario: 程序化缓存过期
- **WHEN** 传入 matcher 与 configured_matcher 不一致或模式已被清除
- **THEN** 接纳后仅按当前配置匹配，不保留旧筛选或扩大无效模式。

#### Scenario: 忽略模式的事件经过 registry
- **WHEN** Ignored 事件带有合法或非法配置模式并经接纳、恢复或刷新
- **THEN** 模式只保留用于显示，matcher 为 None，不因模式跳过该事件。

### Requirement: Restored hook registries validate event identity
HookRegistry 反序列化 SHALL 验证每个 Hook 的 event 与所属 map 键一致，并通过既有 HookSpec.validate 约束；任一失败 SHALL 拒绝整个快照，不静默改派事件或丢弃 handler。

#### Scenario: 事件索引矛盾
- **WHEN** map 键与其中 Hook 的 event 不同
- **THEN** 恢复返回明确错误，不产生可执行 registry。

#### Scenario: 恢复非法失败策略
- **WHEN** 非 admission 事件的 Hook 设置 on_failure block
- **THEN** 恢复返回配置错误。

#### Scenario: 合法多事件快照
- **WHEN** 所有 Hook 与所属事件一致且通过已有校验
- **THEN** 恢复保留事件分组与组内顺序。

### Requirement: Hook shell routing recognizes command separators
命令 Hook SHALL 将包含普通空格、tab 或 LF 的命令文本交由现有 shell 执行；去重 SHALL 使用相同路由判定。其他无 shell 语法的路径仍直接执行。

#### Scenario: Tab 参数与多行命令
- **WHEN** 命令用 tab 分隔参数或 LF 分隔命令
- **THEN** 经 shell 执行，不把整段文本当作相对文件路径。

#### Scenario: 跨来源 shell 去重
- **WHEN** 相同 tab 或 LF 命令来自不同 source_dir
- **THEN** 按 shell 命令既有 first-wins 规则只保留首项。

### Requirement: Skill metadata omits inactive sampling overrides
Skill discovery and RPC metadata SHALL NOT expose unused model or effort override fields. Session model selection and reasoning effort SHALL remain independent.

#### Scenario: Skill metadata advertisement
- **WHEN** a skill is discovered and advertised through RPC
- **THEN** no skill-level model/effort override is advertised.

### Requirement: Skill inventory has one runtime owner
SkillManager SHALL remain the source for merged startup/discovered skill listings and slash advertisement without a parallel AvailableSkills resource snapshot.

#### Scenario: Dynamic discovery retains startup skills
- **WHEN** a new skill is discovered after registry initialization
- **THEN** SkillManager retains both startup and discovered skills in its active listing.

### Requirement: Hook history publication is observational

会话恢复的 Hook 快照 SHALL 仅从已完成 Timeline Hook 事实生成，保留 occurrence 与工具 cause 的身份并明确标记历史快照。发布 SHALL 不重执行历史 handler、不追加 Hook/UI 持久事件、不关闭 rewind 窗口；实时执行保留既有执行与持久化边界。

#### Scenario: 重复发布已完成 Hook
- **WHEN** 一个已有完成 Hook 和可用 rewind 窗口的会话多次发布控制状态快照
- **THEN** 接收端获得相同 occurrence 的历史投影，Timeline 与持久通知不增加，handler 不运行，rewind 窗口保持可用。

证据入口：`crates/codegen/shell/src/session/actor/hook_dispatch.rs`、`crates/codegen/shell/src/session/actor/updates.rs`。

### Requirement: Abandoned MCP calls notify their originating service

Grow 对 stdio/Streamable HTTP MCP `tools/call` SHALL 持有本次请求的 JSON-RPC id 与发起时的 peer。请求在收到回复前因工具期限到期或调用 future 被取消而放弃时，SHALL best-effort 向原 peer 发送匹配 id 的 `notifications/cancelled`，而不是向按服务器名称查得的替代连接发送。已完成调用 SHALL 不发取消；通知失败 SHALL 不修改工具结果。取消通知不承诺 server 停止或撤销副作用，也不授权重放。

#### Scenario: Completed result or business error
- **WHEN** 一次工具调用正常返回结果或 `isError` 业务结果
- **THEN** 守卫解除，不发送取消通知，结果按原语义交付。

#### Scenario: Tool deadline expires
- **WHEN** 在途请求到达其配置期限而没有回复
- **THEN** 原 peer 收到至多一次与该请求 id 匹配的取消通知；Grow 返回说明可能已有副作用的 timeout，HTTP 仅按原服务身份条件重置 transport，不自动重放该工具。

#### Scenario: Turn cancellation drops the call
- **WHEN** 有 request id 的调用 future 在收到回复前被丢弃
- **THEN** 不等待远端处理即 best-effort 发匹配 id 的取消通知；没有 id 时不伪造通知。

#### Scenario: Retry belongs to another request
- **WHEN** 原调用失败后，调用方明确接纳一次新的独立操作
- **THEN** 新旧操作使用各自的 id/peer，任一轮放弃只通知该轮；连接恢复不自动生成第二次工具调用。

#### Scenario: Replacement service or failed notification
- **WHEN** 旧调用被取消时新 Ready 服务已安装，或取消通知发送失败
- **THEN** 新服务不接收旧 id 的通知也不被旧请求重置；通知失败仅留下诊断，原调用终态/取消语义不被覆盖。

#### Scenario: ACP reverse bridge limitation
- **WHEN** `tools/call` 经过当前不转发无 id 通知的 ACP reverse bridge
- **THEN** Grow 不宣称该 bridge 的远端必然收到取消；stdio/HTTP 的协议通知行为不因此退化。

### Requirement: MCP tool results retain structured and typed content

Grow SHALL 对 MCP `CallToolResult` 的成功与 `isError` 结果保留 `structuredContent` 及按原序可解释的 content block，不把只有结构化内容的结果视为空。主模型路径 SHALL 将 text/结构化 JSON 作为受现有文本截断约束的正文，将 image block 和 image MIME 的 embedded resource 作为独立、受图片限额与正规化约束的附件；unsupported block SHALL 有可辨认的类型占位，资源链接不得被隐式抓取。direct `grow/mcp/call` SHALL 返回有界的完整原生结果，而非丢字段的 `{type,text}` 投影。

#### Scenario: Structured-only and nonduplicate JSON
- **WHEN** 工具只返回 `structuredContent`，或同时返回摘要 text 与不等价的结构化 JSON
- **THEN** 主模型结果包含完整结构化语义（超长时遵守现有预览/文件保存），摘要及不同 JSON 均可见；与结构化 JSON 语义相等的 text 不重复展示。

#### Scenario: Business error with nontext content
- **WHEN** `isError` 为 true 且含结构化内容、图片或资源信息
- **THEN** 工具仍标记失败，但上述内容遵守各自投影规则，不被仅保留 text 的错误分支丢弃。

#### Scenario: Image survives text truncation
- **WHEN** 前置正文超过 MCP 文本限额，后面跟有效 image block 或 image resource
- **THEN** 正文按既有规则截断/保存，图片不被切成半截 data URI，仍在图片限额内形成视觉附件并按原结果顺序交付。

#### Scenario: Invalid or excessive image and unsupported blocks
- **WHEN** 图片 MIME/base64/解码无效或超出单件、数量预算，或结果包含当前模型不能呈现的 audio/未知 block
- **THEN** 有明确的省略/不可呈现说明，不向模型发送损坏的图片或未受控的原始字节，不静默丢 block。

#### Scenario: Complete direct result
- **WHEN** `grow/mcp/call` 收到 text、image、audio、resource、resource link 混合结果及 `structuredContent`/`isError`/`_meta`
- **THEN** 有界响应保留原始 block 顺序、类型和字段；不会把 image 丢弃或 resource 伪装成 text。

#### Scenario: Direct response exceeds its budget
- **WHEN** 完整 direct 结果的编码大小超出明确响应上限
- **THEN** extension 返回可识别的超限错误，不发送不完整却声称成功的结果。

### Requirement: Live ACP sessions refresh their MCP catalog from configuration changes
Pager 进程内 ACP session SHALL 接收适用的 `McpCatalogChanged` 并通过既有 Shell catalog reload 路径刷新其 MCP server 集合。刷新 SHALL 使用 session 实际 cwd 和现有 `mcp_recursive_config_watch` 配置控制项目监听；配置合并、信任过滤与客户端 MCP 配置优先级保持现有语义。重复相同配置 SHALL 幂等；关闭项目监听 SHALL 不登记项目路径，进程取消 SHALL 结束该运行时的配置监听任务。

#### Scenario: Global MCP server is added after session startup
- **WHEN** 活动 ACP session 启动后新增全局 MCP server 配置
- **THEN** session 收到 catalog 刷新并可 initialize/list 及调用该 server 的工具，无需重启 session。

#### Scenario: Project MCP configuration changes for an active session
- **WHEN** 已启用项目监听的 session cwd 下新增或修改项目 MCP 配置
- **THEN** 仅适用该 session 的 catalog 合并并刷新，已有目录信任过滤继续生效。

#### Scenario: Project configuration directory is created later
- **WHEN** session cwd 对应的 `.grow` 配置目录在 watcher 启动时不存在，之后创建并加入 MCP 配置
- **THEN** 延迟目录监听发现变化并刷新该 session 的 catalog。

#### Scenario: A session starts with a different cwd
- **WHEN** Pager 动态建立 cwd 不同的 session
- **THEN** watcher 为该实际 cwd 注册项目配置监听，刷新范围按该 cwd 解析。

#### Scenario: Session cwd and filesystem event use directory aliases
- **WHEN** session cwd 经过符号链接或平台目录别名，而 filesystem event 使用同一目录的规范路径
- **THEN** 延迟项目目录监听及 reload 范围匹配仍识别该 session，且不匹配其他目录。

#### Scenario: Identical catalog update is repeated
- **WHEN** 配置重载重复交付同一份有效 MCP catalog
- **THEN** session 保持同一有效 catalog，不重复注册或破坏已初始化的等价 server。

#### Scenario: Recursive project watching is disabled
- **WHEN** `mcp_recursive_config_watch` 解析为 disabled
- **THEN** 不注册项目 cwd watcher，且不产生未消费的动态路径登记；适用的 global reload 仍可工作。

#### Scenario: Client MCP configuration wins for the same merge key
- **WHEN** 刷新 catalog 时目录配置与 ACP client 配置使用相同目录合并键
- **THEN** 继续采用既有客户端配置优先级并保留其连接与工具。

#### Scenario: In-process watcher is cancelled
- **WHEN** Pager 进程取消或退出
- **THEN** 配置 watcher、动态 cwd drain 与 reload consumer 均结束，不遗留后台任务。

### Requirement: MCP catalog lifecycle follows the effective configuration

An ACP session's advertised MCP tools, callable MCP tools, and owned server connections SHALL converge on the session's current effective merged catalog after configuration changes. Removal or replacement SHALL retire the superseded server and its tools. An asynchronous initialize or reconnect result started for an older configuration SHALL NOT restore a removed or replaced server. Catalog changes SHALL remain scoped to the sessions and cwd for which the existing merge rules make the configuration applicable. Cold recovery SHALL rebuild from current applicable configuration and preserve client-source behavior supported by the existing session recovery semantics; enabled project watching SHALL resume for the recovered session's cwd.

#### Scenario: Removed server is no longer listed or callable
- **WHEN** an applicable configuration update removes an MCP server from a live session's effective catalog
- **THEN** the session no longer advertises that server's tools, calls cannot use its retired connection, and unrelated servers remain available.

#### Scenario: Replaced server converges on the replacement
- **WHEN** an applicable configuration update replaces a server while retaining its catalog identity
- **THEN** the session advertises and calls through the replacement configuration, and the superseded connection and tools are retired.

#### Scenario: Stale connection work completes after removal or replacement
- **WHEN** initialize or reconnect work for an old server configuration completes after a newer catalog update removed or replaced it
- **THEN** the stale result cannot restore the old connection, readiness, configuration, or tools.

#### Scenario: A direct call names a retired endpoint
- **WHEN** a direct MCP call includes an explicit server URL that is no longer in the session's effective catalog
- **THEN** it is rejected without routing to a same-name server at another URL.

#### Scenario: Recovery or list notification changes tool definitions
- **WHEN** the enabled session event dispatcher observes a current connection becoming ready after recovery or publishing a tools-list change
- **THEN** it refreshes the server's complete tool definitions and search snapshot, including an empty tool list, even if automatic transport recovery is disabled.

#### Scenario: Session dispatcher stops during catalog discovery
- **WHEN** the session event dispatcher is stopped while its catalog discovery is pending
- **THEN** its pending discovery tasks are cancelled and cannot later publish a catalog or continue discovery for another session.

#### Scenario: A retired client's call fails late
- **WHEN** an already-dispatched call reports a recoverable failure after its client was removed or replaced
- **THEN** it returns the failure without reconnecting or retrying the retired client and without routing the call to the replacement.

#### Scenario: Catalog changes remain isolated by session cwd
- **WHEN** two sessions use different cwd values and a project MCP configuration changes for one cwd
- **THEN** only sessions applicable to that project configuration update their catalog; the other session's listed and callable tools remain unchanged.

#### Scenario: A nested session consumes an ancestor project configuration
- **WHEN** a session's configuration merge reads an ancestor directory up to its repository root
- **THEN** the same directory chain is watched for configuration creation, replacement, and deletion; removing another cwd's registration does not remove shared ancestor watches still needed by the session.

#### Scenario: Cold recovery rebuilds from current configuration
- **WHEN** an ACP session is recreated after its prior runtime has stopped and applicable MCP configuration changed while it was stopped
- **THEN** recovery builds the effective catalog from current applicable configuration, retains client-source behavior provided by the existing recovery inputs and precedence rules, and restores project watching when enabled for that session cwd.

### Requirement: Live MCP auxiliary policy follows effective configuration

An existing session's effective MCP policy SHALL follow applicable configuration changes to `disabled_mcp_tools`, `startup_timeout_sec`, `tool_timeout_sec`, and `tool_timeouts`. Removing an override SHALL expose the remaining lower-priority setting or that field's default. Policy assembly SHALL retain existing metadata precedence and configuration trust/cwd applicability. Asynchronous work started under older policy SHALL NOT commit that policy after a newer effective policy is accepted. A call already admitted SHALL retain the deadline selected for that call; later policy changes apply to subsequently admitted calls.

Subagent sessions SHALL refresh their local disabled-tool gate and bridge registrations when effective global or trusted project policy changes, while ignoring client MCP catalog replacements. A subagent SHALL NOT mutate timeout policy on inherited shared clients; only the owning session refreshes those clients. Internal configuration reload SHALL enqueue this policy refresh for every active child whose cwd matches the reload scope, including workflow children, without replacing the child's frozen catalog or shared-client ownership.

#### Scenario: Existing session receives policy changes
- **WHEN** applicable configuration changes one or more of `disabled_mcp_tools`, `startup_timeout_sec`, `tool_timeout_sec`, or `tool_timeouts`
- **THEN** the existing session applies the new effective policy without requiring session recreation

#### Scenario: Policy override is removed
- **WHEN** an applicable configuration source removes an override for one of the policy fields
- **THEN** the session uses that field's default instead of retaining the previously effective override

#### Scenario: Metadata precedence is preserved
- **WHEN** a policy value from metadata and another applicable policy source provide competing values
- **THEN** the established metadata precedence determines the effective value

#### Scenario: Project policy is untrusted or outside session scope
- **WHEN** a project policy is not trusted under existing rules or does not apply to a session's cwd
- **THEN** that policy does not affect the session

#### Scenario: Older asynchronous work finishes after policy refresh
- **WHEN** initialization or reconnect started under an older policy completes after a newer effective policy is accepted
- **THEN** the older work cannot restore the old policy

#### Scenario: In-flight call crosses a policy update
- **WHEN** a call has been admitted with a timeout and the effective policy changes before that call completes
- **THEN** the in-flight call retains its admitted deadline and a subsequently admitted call uses the updated policy

#### Scenario: Policy changes while an admitted call waits for initialization
- **WHEN** a call begins initialization under one effective timeout and policy changes before initialization completes
- **THEN** initialization and the call remain bounded by the deadline captured at admission, and the new policy applies to later calls

#### Scenario: Subagent refreshes its local disabled gate
- **WHEN** an applicable global or trusted project disabled-tool policy changes while a subagent is active
- **THEN** the subagent reconciles its local bridge and runtime gate without accepting a client MCP catalog replacement or changing timeout policy on inherited shared clients

#### Scenario: Cold recovery reads current auxiliary policy
- **WHEN** a session runtime is recreated after its applicable disabled-tool or timeout configuration changed while stopped
- **THEN** it installs the current effective policy and does not recover an older cached mask or timeout from session history.

#### Scenario: Subagent policy refresh preserves inherited ownership
- **WHEN** an existing subagent receives a policy-only refresh while using inherited parent MCP clients
- **THEN** it reconciles its own applicable disabled-tool mask without accepting replacement client catalogs or mutating policy on shared parent clients.

#### Scenario: Internal reload reaches active child policy
- **WHEN** an internal configuration reload applies to an active child session's cwd
- **THEN** the child recomputes its local effective auxiliary policy through its actor command path
- **AND** its inherited MCP catalog and parent-owned shared client policy remain unchanged

#### Scenario: Scoped reload skips unrelated child
- **WHEN** an internal configuration reload names a project root that does not contain an active child's cwd
- **THEN** that child receives no policy refresh command

### Requirement: MCP server name is the session identity

Within one session, an MCP server's `name` SHALL be the single identity used by its catalog directory entry, client, and tool-name prefix. Configuration SHALL preserve the established source precedence, including configuration over plugin and client precedence where applicable. When same-name entries from different sources refer to different endpoints, the catalog SHALL select one authoritative entry deterministically according to that precedence and SHALL NOT combine identity from one entry with the endpoint or client of another. A single client-provided server array containing duplicate names SHALL be rejected.

#### Scenario: Same name appears across sources with different endpoints
- **WHEN** applicable configuration sources provide the same server name with different endpoints
- **THEN** one authoritative entry is selected by the existing source precedence and its name, client, endpoint, and tool prefix remain associated

#### Scenario: Client array repeats a server name
- **WHEN** one client-provided server array contains the same server name more than once
- **THEN** the array is rejected with a clear validation error and no ambiguous catalog is published

### Requirement: Explicit MCP call identity matches the current catalog

A direct MCP call that supplies both server `name` and URL SHALL be accepted only when both identify the same current authoritative catalog entry. A URL match SHALL NOT redirect a mismatched name to a different server. The identity check SHALL continue to apply across cold recovery and hot replacement, so calls and published tools cannot use a stale or mixed identity.

#### Scenario: Explicit name and URL identify different entries
- **WHEN** a direct call provides a name and URL that do not belong to the same current authoritative catalog entry
- **THEN** the call is rejected without routing to either entry by partial match

#### Scenario: Explicit name and URL match the current entry
- **WHEN** a direct call provides the current authoritative entry's name and URL
- **THEN** the call is routed through that entry's client

#### Scenario: Cold recovery rebuilds server identity
- **WHEN** a session recovers after its previous runtime stopped
- **THEN** each catalog name, client, endpoint, and tool prefix are rebuilt from the same current authoritative entry

#### Scenario: Hot replacement completes while old work is pending
- **WHEN** a same-name server is replaced while initialization or call-discovery work for the previous endpoint is pending
- **THEN** the old work cannot publish old tools as current or route a new call through the previous endpoint

#### Scenario: Configuration mutation shares catalog authority
- **WHEN** setup, toggle, upsert, or delete changes a disk definition whose name also exists in client input
- **THEN** the live session applies the complete merged catalog using the same source precedence as reload, and its displayed configuration and invocation use that same authoritative entry.

### Requirement: Uncertain MCP tool calls are never automatically replayed

Grow SHALL dispatch an ordinary MCP `tools/call` at most once after invocation begins unless it has explicit evidence that the tool request was never sent. Transport loss, HTTP MCP errors, timeouts, identical arguments, and tool annotations SHALL NOT independently authorize replay. An uncertain failure SHALL explain that effects may already have occurred. Connection recovery MAY prepare a later independent call and SHALL remain conditional on the originating service identity.

#### Scenario: Server writes before its reply is lost
- **WHEN** a server applies a tool effect and the invocation loses its transport or reply
- **THEN** Grow reports an uncertain outcome without sending a second tool request, and the server effect count remains one.

#### Scenario: Later explicit call uses recovered connectivity
- **WHEN** a failed originating transport has been invalidated and a new independent tool invocation is admitted
- **THEN** existing initialization may reconnect for that new invocation without replaying the previous one.

#### Scenario: Replacement and annotations do not grant replay
- **WHEN** an old invocation fails after replacement, or its tool advertises read-only or idempotent hints
- **THEN** Grow neither replays it on the old or replacement client nor resets the replacement using the old failure.

### Requirement: Explicit skill loading shares one runtime operation
用户斜杠技能调用与 Agent 工具调用 SHALL 使用同一技能加载操作，复用唯一运行时目录和正文准备规则。显式用户调用 SHALL 由 Runtime 派发，不依赖前置模型决定。触发来源 SHALL 由 Runtime 持有，不接受调用参数伪造。

#### Scenario: User and model load the same skill
- **WHEN** 用户斜杠命令与 Agent 分别请求同一有效来源和参数
- **THEN** 两者使用相同加载语义和结果结构；用户路径无需前置任务模型决定是否加载，来源分别标记为用户与模型，现有权限辅助调用仍按原管线执行。

#### Scenario: Model attempts to claim user invocation
- **WHEN** 模型试图通过加载参数声明用户来源或权限批准
- **THEN** Runtime 不接受该声明，仍按模型调用资格和权限处理。

#### Scenario: Read and initialization remain distinct
- **WHEN** Agent 通过文件工具审阅 SKILL.md，或初始化阶段处理 Agent 的 skills 声明
- **THEN** 文件读取不构成显式加载，初始化保持预加载角色；两者都不伪造用户斜杠加载记录。

证据入口：`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` 的 SkillManager、`tools/src/implementations/skills/skill.rs` 的内容准备、`shell/src/session/slash_commands.rs`、`agent/src/prompt/skills.rs`（后面三项均位于 `crates/codegen/`）。本条是新增契约。

### Requirement: Skill load eligibility follows its actual caller
加载 SHALL 复核已选条目的 enabled、条件可见性和调用资格。用户入口 SHALL 遵守 user_invocable；模型入口 SHALL 遵守 disable_model_invocation；不得将一个入口的过滤结果冒充另一个入口的可调用目录。

#### Scenario: User-only skill
- **WHEN** 技能允许用户调用且禁止模型自主调用
- **THEN** 用户显式调用可进入加载门控，模型调用被明确拒绝。

#### Scenario: Model-only skill
- **WHEN** 技能禁止用户调用但允许模型调用，且满足其余门控
- **THEN** 模型可加载，用户入口不把它作为可调用斜杠技能。

#### Scenario: Catalog changes before execution
- **WHEN** 已接纳引用的来源在消费前禁用、不可见或失去调用资格
- **THEN** 加载失败，不使用旧目录可见性绕过当前门控。

证据入口：`crates/codegen/shell/src/session/slash_commands.rs` 的 EffectiveCommandCatalog、`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` 的模型列表过滤。

### Requirement: Skill invocation resolves identity before eligibility
Grow SHALL resolve a skill name against its current catalog before checking enabled and caller eligibility. A disabled selected identity SHALL produce an explicit cannot-load error and SHALL NOT fall back to another source. Model guidance SHALL require load_skill for using skills and require reporting failure without bypass through file reads, shell commands, or alternate copies. Ordinary file inspection SHALL remain distinct from activation.

#### Scenario: Disabled name collides with an enabled plugin
- **WHEN** a disabled native skill shares a bare name with an enabled plugin skill
- **THEN** the bare name is ambiguous, the disabled qualified name fails explicitly, and only explicitly selecting the enabled qualified name can load it.

#### Scenario: Disabled or unavailable skill is requested by a role
- **WHEN** a model requests a disabled, missing, or caller-ineligible skill
- **THEN** the loader reports why it cannot load the skill, publishes no guidance, and instructs the model to report the failure without attempting another access route.

### Requirement: Skill discovery advertises names without eager bodies
Grow SHALL use its existing startup and refresh catalog as the only skill index. Model-facing discovery SHALL advertise names and bounded descriptions without installation paths or eagerly loaded bodies. Names SHALL remain usable by load_skill without supplying a filesystem path.

#### Scenario: Large skill with optional references
- **WHEN** discovery indexes a skill with a long body and several reference documents
- **THEN** the catalog contains only its bounded metadata and the resources are not loaded into model context.

#### Scenario: Preloaded skill needs a reference
- **WHEN** an Agent initialization declaration loads a skill body
- **THEN** its catalog identity remains available for later name-based resource loads.

### Requirement: Skill resources load progressively through the same tool
load_skill SHALL accept an optional resource relative to the selected skill root. Without resource it SHALL load only the main skill instructions. With resource it SHALL load only that UTF-8 file through normal authorization, cancellation and durable SkillLoad publication, without executing scripts, expanding templates, or recursively loading references. Resources SHALL respect the same enabled/caller policy, 1 MiB source bound and gitignore policy as main loads, and SHALL reject args combined with resource.

#### Scenario: Main instructions followed by a needed reference
- **WHEN** the model first calls load_skill by name and later requests references/api.md
- **THEN** the first result contains only the main instructions and the second contains only that reference with its skill/resource identity; neither includes sibling files.

#### Scenario: Resource cannot be combined with template arguments
- **WHEN** a load_skill request supplies both a resource and args
- **THEN** the request fails before reading or publishing either body; resource reads do not apply skill template arguments.

#### Scenario: Invalid or escaping resource
- **WHEN** resource is absolute, contains parent traversal, has a Windows drive or UNC spelling, or resolves through a symlink outside the skill root
- **THEN** loading fails with no resource guidance published and no fallback path read.

#### Scenario: Resource permissions and file identity
- **WHEN** a resource request reaches authorization or its resolved path changes before execution
- **THEN** authorization covers the actual resource file and changed identity fails before reading instead of reusing permission for SKILL.md.

#### Scenario: Missing, oversized, non-UTF-8 or cancelled resource
- **WHEN** the resource is unavailable, exceeds 1 MiB, is not UTF-8 or preparation is cancelled
- **THEN** loading fails without publishing a partial body or claiming Loaded.

#### Scenario: Skill has no accessible resource root
- **WHEN** a resource is requested from a URI-backed or frozen-only skill without an accessible local resource root
- **THEN** loading returns an explicit resource-unavailable error without resolving a fallback path or publishing content.

#### Scenario: Resource result survives suspension
- **WHEN** a resource result was durably admitted before suspension
- **THEN** replay uses the committed resource body and existing SkillLoad identity without reopening the current file or recursively loading more material.

### Requirement: Skill file inspection follows ordinary read bounds
Reading SKILL.md or Markdown under a skills directory through a file tool SHALL remain ordinary file inspection, honoring requested line windows and the normal token cap. Full skill guidance SHALL be loaded through load_skill and its explicit preparation/publication boundaries.

#### Scenario: Inspect a line of a skill for editing
- **WHEN** a file read requests a line window of SKILL.md or a reference document
- **THEN** only the requested bounded window is returned and no Skill Loaded fact is created.

#### Scenario: Oversized skill file inspection
- **WHEN** an ordinary skill file read exceeds the normal token cap
- **THEN** the same size error as other text files is returned; the skill filename does not bypass read limits.

#### Scenario: Resource call crosses the durable publication boundary
- **WHEN** a successful resource load is committed for a model-issued load_skill call
- **THEN** Timeline accepts the resource argument only when the current call matches the uniquely admitted call, including the resource; unknown fields or a changed resource cannot publish guidance.
