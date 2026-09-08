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
