# shell

Grow 的会话、配置、模型路由与工具运行时核心。

用户文档、BYOK TOML 示例、Agent 规则和构建方式统一维护在仓库根目录的
[`README.md`](../../../../README.md)，避免子 crate 文档复制过时的上游安装、认证与网络端点。

本 crate 不内置模型。模型提供商、模型名、API 端点和密钥均来自用户配置；未配置可用
LLM 时，由前端引导用户打开配置。

设置保存仅将不存在的配置文件视为空配置；读取错误（包括非法 UTF-8）或 TOML 语法错误会终止保存，保留原文件。读改写操作也在初始加载失败时返回错误，不执行设置修改。

设置读改写读取原始 TOML，不先展开环境变量或应用版本覆盖，避免修改无关字段时把运行时结果写回基础配置。运行时配置加载仍执行原有解析。

设置编辑会严格解析将要写回的配置段；字段类型不合法时返回段名与错误，保留原文件，不以默认值覆盖。缺失段仍可创建，合法段中的未知字段仍保留。

技能已知设置全部清空时，仅清除当前认识的字段；未知字段或子表继续保留，真正空的 `[skills]` 段才会删除。

设置读改写会比较修改前后的已知字段，明确设为 `None` 的可选值会从磁盘删除，恢复默认不再留下旧值；未知嵌套字段仍保留。

配置原子写入使用同目录独占临时文件，失败自动清理。Unix 新配置默认仅所有者读写；既有文件权限在写入内容前应用，权限错误会终止保存。此路径不提供断电后的持久性保证。

## Session ownership

`SessionActor` 是会话的单一组合根，并作为 `!Send` actor 运行在 Tokio
`LocalSet` 上。它串行协调 ACP mailbox、Timeline 提交、foreground turn、工具调用、
通知和 Goal continuation；这些事实不得复制到第二个 actor 或前端状态中。

- `AdmissionState` 的单一 `TokioMutex` 同时保护 foreground owner、用户 FIFO、
  manual compaction admission、notification suppression 和 rewindable boundary。
  不得把这些字段拆成多个锁；任何调用都不得持有 admission guard 跨未知 `.await`。
- `McpSessionState` 只聚合会话级 MCP 策略、初始 server 配置、tool metadata、
  announcement/reminder 与 readiness 状态。foreground、Timeline notification 和 MCP
  进程生命周期仍由既有 owner 管理。
- `HookSessionState` 只聚合 hook registry、client hooks、workspace/VCS context 和加载
  错误。Plugin registry 保持独立，Hook 与 MCP 不合并为通用 extensions bag。
- `BehaviorCoordinator`、`GoalTracker`、`WorkflowManager`、`SessionMemory`、
  `EventTracker` 与 ChatState/Timeline 各自拥有其领域状态；`SessionActor` 只负责按既定
  顺序协调它们。

Idle admission 的优先级是用户 FIFO → durable notification → Goal continuation。
Timeline 是唯一持久事实源；foreground、projection 或 render cache 都不能成为第二份
持久事实。`RefCell` borrow、MCP/admission lock 以及 hook registry borrow 均不得跨未知
`.await`。

## Dependency direction

协议、权限、工具、MCP 和持久化能力可以被 session actor 调用，但不得反向依赖
`SessionActor` 的内部字段。工具调用的授权与 dispatch 继续围绕
`PreparedToolCall` / `ToolDispatchAuthority`，不会引入第二个 tool runtime 或 session
actor。

## LSP configuration sources

LSP 执行配置在合并前应用项目信任许可：未信任的 `.grow/lsp.json` 不参与同名覆盖，用户和允许插件的配置仍可使用；已信任项目保持覆盖优先级。执行前的来源过滤继续复核信任。`grow inspect` 使用报告的同一信任结果展示允许来源，同时另列被禁用的项目定义。同名时允许来源排在前面，项目项标记为 untrusted；这些条目不是运行中进程清单，不能由显示结果推导执行授权。配置合并契约见 [configuration-rules](../../../openspec/specs/configuration-rules/spec.md)，诊断展示契约见 [client-surfaces](../../../openspec/specs/client-surfaces/spec.md)。

LSP 诊断的允许来源从现有插件注册表的 active_plugins 取得；禁用或未信任插件的定义单独列出，分别标记 disabled 与 untrusted。同名允许项优先展示，受限定义不会参与覆盖。执行端和诊断端共用插件文件/inline 解析规则。

## Web fetch model budget

模型切换更新 WebFetchClient 的窗口参数。客户端克隆共享完整文本缓存，但缓存命中仍按各调用的窗口生成预览；裁剪时的恢复文件属于当前会话，不能写回共享缓存。这样旧路由中的调用和之后放大窗口的调用仍能取得完整文本。契约见 [model-sampling](../../../openspec/specs/model-sampling/spec.md)。

web_fetch 的 `max_cache_entries = 0` 关闭文本缓存；非零容量下，更新同一 URL 不驱逐其他条目，仅新增 URL 且容量已满时淘汰最早插入项。容量契约见 [configuration-rules](../../../openspec/specs/configuration-rules/spec.md)。

web_fetch 在逐块读取解码后的 HTTP 正文时检查 `max_content_length`，超限立即返回大小错误，不等待响应结束；恰好上限正常处理。该限制约束累计正文长度，不包含 HTTP 库单块缓冲和后续解析开销。

web_fetch 静态允许列表的主机与路径分别处理：主机沿用大小写、www 和尾点规范化；路径区分大小写并保留末尾点，只允许完整路径段前缀。未匹配仅表示不获得该静态条目的许可，后续仍由权限管理策略决定。契约见 [tool-authorization](../../../openspec/specs/tool-authorization/spec.md)。

web_fetch 只请求当前调用的 URL。所有 HTTP 跳转（包括同主机）返回 `RedirectRequired` 和解析后的目标 URL，由新工具调用经过现有授权后获取；客户端不把原 URL 的许可扩展到其他路径或端口。目标语法、凭据和 scheme 在返回前检查，DNS/SSRF 在目标的新调用中检查。ACP 将未获取内容的跳转结果标记 Failed，并保留新调用提示。

HTML/XHTML 与 PDF 分支只匹配 Content-Type 的实际媒体类型，忽略类型大小写；参数中的类型字符串不会触发转换或文件保存。具体格式契约见 [client-surfaces](../../../openspec/specs/client-surfaces/spec.md)。

## MCP recovery task ownership

stdio 重启与 HTTP 恢复在取得 in-flight 去重标记后、spawn_local 前创建释放守卫。任务 future 自创建起持有守卫，因此 LocalSet 在首次 poll 前销毁任务也会释放标记；运行中的任务继续在完成、取消或异常退出时释放。契约见 [extension-runtime](../../../openspec/specs/extension-runtime/spec.md)。

MCP 恢复在调度与重试循环的配置探测等待中也监听取消。取消直接结束，不将取消误报为服务器 Disabled；循环持有的去重标记仍由守卫释放。这覆盖异步探测等待，不中断探测内部同步文件读取。

Dispatcher 自身持有恢复 token 的 drop guard。fatal 或超时 abort 跳过正常退出代码时，future 销毁仍会取消独立恢复任务；正常关闭继续主动取消并等待去重标记清理。强制终止只保证发出取消，不将 abort 等同同步 join 所有恢复任务。

MCP liveness 旧任务在状态检查后清理槽位时，必须在槽位锁内复核自身 token。替换句柄会在同一锁内取消旧 token，因此迟到的旧任务不会取走或取消新监听器，也不会发送该次过期关闭事件。

MCP 并发 recover 在同一状态锁内判定 Ready 并重置到 Pending。竞争调用观察到 Pending/Initializing 后加入已有握手，不再次覆盖 transport；复用 ensure_initialized 的单次握手机制。

工具错误触发 MCP 恢复时传入失败调用所用的服务 Arc。若当前 Ready 已被替换，就复用新服务完成原有的一次重试，不再重置新连接；主动恢复继续使用独立入口。

MCP 超时分支同样将 reset 绑定到该调用的服务身份，在状态锁内核对并修改。已替换的 Ready 或进行中的握手不被旧超时覆盖；超时仍返回错误，不自动重放可能有副作用的工具调用。

stdio 重启把配置失效与真实失败分开处理。配置已移除，或启动/握手返回后发现 generation/配置内容变化时，旧任务返回 Superseded 并结束，不推送失败或耗尽状态、不注销工具；普通失败保留原有重试与耗尽处理。

MCP liveness 任务在检查间只持有客户端弱引用，避免从配置移除的空闲连接被监听任务保活。客户端释放后，监听任务在下一次检查清理自己的槽位并静默退出；正在执行的检查仍可短暂持有客户端。

MCP stdio 重启在监听器初始化后再次核对已安装客户端身份，核对与工具刷新使用同一状态锁；配置替换后的旧任务静默结束，不借用新客户端身份报告刷新或重启成功。

HTTP MCP 恢复在处理握手结果前，同时复核当前连接身份与 HTTP 配置；配置失效使用共享 RecoveryError::Superseded 结束旧循环，不在下一次重试中重置同名替代连接。真实传输故障保留原退避重试策略。

MCP 初始化的取消恢复守卫覆盖握手结果提交时的状态锁等待；取得锁后才撤销守卫并同步写入结果，避免等待提交期间取消遗留 Initializing。

stdio MCP 握手取消后，若状态锁可取得，客户端转为 Empty 并唤醒初始化等待者；后续调用立即报告 transport 不可用。可重建的 HTTP/ACP 取消后仍恢复 Pending。

MCP 客户端私有状态使用短时同步锁，握手和通知等待均在锁外；初始化取消会在竞争结束后恢复 Pending/Empty，不再因 try_lock 失败遗留 Initializing。会话 McpState 仍使用异步锁。

命令 Hook 的 stdout/stderr 在读取期间各只保留 64 KiB 前缀和一个截断检测字节，超出部分继续排空。输出缓存不再随总输出量增长，原截断标记、并发 stdin 写入和执行超时保持。

阻塞型 HTTP Hook 正文限制为 64 KiB，分块读取超限立即返回失败并走既有失败策略，不解析截断决策或保存部分预览；Observe 模式仍只检查状态码。

HTTP Hook 请求固定使用 URL 校验阶段通过检查的地址集合，保留原 URL 的 Host/TLS 身份。为避免代理端重新解析绕过本地检查，Hook 客户端不自动使用系统或环境代理，采用直连；IPv6 字面地址直接按 IP 校验。

HTTP Hook 的 timeout_ms 是 URL 校验、请求与正文等待共享的总预算，阶段切换不重新计时。超时返回保留已取得的 URL 和状态码，不保存未完成正文预览；同步代码不提供抢占式中断。

命令 Hook 在进程组建立成功时，通过执行期守卫在取消、超时和 IO 失败后终止同组后台进程，不依赖 session scope。组创建失败仍记录警告并使用直接子进程 kill_on_drop；独立建立新进程组的后代不在此保证内。

Prompt/Tool Hook 的 JSON allow 仅在命令 exit 0 或 HTTP 2xx 时接受，不能掩盖执行失败；其余失败按 on_failure 处理。显式 deny/block 及命令 exit 2 保持拒绝优先。

Hook 决策协议使用 JSON 对象：字段/schema 错误、截断对象或数组输出均记录 Failed，按配置的失败策略处理；空输出和普通文本保留既有行为。Prompt/Tool 命令 exit 2 的拒绝不被正文解析错误覆盖。

Hook 去重在缺失 command_raw/url_raw 展示来源时回退到实际命令/URL，不再将不同程序化配置折叠为空键；相同内容仍保留高优先级的首项。

Hook 去重会区分不同来源目录中的同名直接相对脚本；shell 命令与绝对路径仍按原文本规则去重。执行和去重共用同一个 shell 路由判定，避免目录规则漂移。

恢复时编译失败的 Hook 匹配器始终不匹配，即使事件没有 match_value 也不会运行；正常匹配器对无字段事件的原规则保持。

HookRegistry 在反序列化、追加和去重接纳时主动按事件策略从 configured_matcher 重建匹配器：Ignored 事件只保留模式原文并清除缓存；Tested 事件非法模式保持 Never，清除模式则清除缓存。调用方不必在 wire 恢复后额外重编译。

HookRegistry 恢复还会校验事件索引与每个 Hook 的 event 一致，并执行已有失败策略校验。矛盾或非法快照返回错误，不静默改派事件；见 [扩展运行时规范](../../../openspec/specs/extension-runtime/spec.md)。

命令 Hook 的 shell 路由识别空格、tab 和 LF 分隔符，因此 tab 参数和多行命令不会被当作直接路径。去重共用此判定。

Session recap 的 client、model 与 context_window 预算来自同一次完整配置准备。生成期间切换模型不会将新模型拼入旧 endpoint；后续 recap 使用新配置。见 [采样规范](../../../openspec/specs/model-sampling/spec.md)。

`/btw` 同样从完整准备配置绑定 client 与请求模型，并在既有重试中复用该快照；会话模型切换不改写已准备的旁路请求。

AI Suggest 和 Prompt Suggest 后台生成会监听结果接收端关闭：调用超时或被丢弃后取消生成并释放 activity；已打开的 Sideband 通过原有 Drop 路径记录取消。接收端提前关闭时不启动生成。

ask_user_question 在通知 Hook 阶段取消单个问题时会释放当前 pending guard 并继续服务后续问题，不关闭整个问答 coordinator。服务关闭和 Hook 失败仍按原规则退出。

ask_user_question 在发送前拒绝同题重复选项 label，避免按 label 返回时混淆不同选项或错映射 ID；跨题重复 label 仍允许。

已注册 cwd 的配置 watcher 会在 `.grow` 晚创建或重建时补挂非递归监听，并先发出 config.toml 重读事件，再由 reloader 做内容去重。补挂与 unwatch 共用锁，已取消注册的 cwd 不会被目录事件重新挂载；leader 与内嵌 Pager 复用该实现。

Discovery watcher 保留非递归项目父目录监听，并按目录实体身份恢复被替换的 `.grow`、skills、commands、workflows 注册。普通文件编辑不重新挂载递归树；实体句柄仅保留在这些有限的刷新根上。

技能扩展添加路径时，仅清除同路径及祖先、后代的 ignore；来源和新增数量也按路径组件计数。`foo` 不包含相邻的 `foobar`。契约见 [configuration-rules](../../../openspec/specs/configuration-rules/spec.md)。

技能管理比较路径时展开 tilde 并解析现存符号链接，添加去重、ignore 清理、移除和来源计数使用同一解析方式；保留条目的配置原文不被重写。请求使用请求 cwd，已有配置仍相对进程 cwd 解析。

技能管理对原始配置中的环境变量引用先按运行时规则展开再比较，保留条目的原文不变。该步骤只用于配置值；普通请求路径与已经展开的来源列表不额外展开。

技能请求使用相对 cwd 时，即使目标尚不存在，也先锚定到进程工作目录再尝试 canonicalize；保存的新增路径不会仅因目标尚未创建而保持相对形式。

技能 reset/config 的可选 cwd 在操作前严格检查类型，无效请求返回 invalid_params；空对象和 cwd:null 保持默认目录语义。

技能扩展重载将配置读取与技能扫描放入 blocking worker，扩展内共享一个执行许可。5 秒截止时间包含排队；超时只停止等待，许可保留至 worker 真正退出，避免累积扫描。重载失败返回错误；若配置已保存，响应明确说明保存已完成。正常空目录仍返回成功空列表。该边界不覆盖其他 session/inspect/workflow 发现调用，也不是整个管理请求的总截止时间。

配置技能的 Repo/User 分类依据配置根与仓库根的规范路径包含关系，符号链接别名不会单独改变来源优先级；扫描输入和配置原文保持原样。

自动技能发现从规范 cwd 向规范 Git root 遍历，在仓库边界停止；外部 cwd 链接的祖先不会混入项目来源。本地 .grow 即使链接到共享目录，仍按本地入口赋予 Local scope。

共享技能扫描按当前祖先链的规范目录身份跳过回环链接，在加入技能文件之前检查；不同非祖先别名仍保留词典序，交由后续文件身份去重。

Server/Bundled 配置注入目录与插件、配置技能目录共用根 SKILL.md 加递归子目录的发现语义，保留来源优先级与规范文件去重。

技能描述回退只读取 frontmatter 预算加正文预览预算（额外一个探测字节），不再为了短描述读取完整正文；UTF-8 截断保留完整字符。显式正文加载不套用该预览限额。

已识别 opening fence 的技能 frontmatter 超过读取预算时会报错并跳过技能，不能退化成默认限制的普通技能；无 frontmatter 长正文仍可发现。

技能发现跳过未闭合 frontmatter 和 YAML 语法错误，不能以默认元数据替代损坏的限制声明。合法无 frontmatter Markdown 的名称和描述回退保持不变。

技能 paths 类型错误或混合列表会导致该技能解析失败，不能丢弃错误元素后变成无条件技能；合法字符串、字符串列表及既有空/匹配全部语义保留。

技能调用开关仅接受布尔值或 true/false 字符串；错误类型拒绝加载，不能将错误禁用开关变成 false。字段缺省默认保持不变。

技能展开提示的 skills_referenced 索引仅记录成功加载并生成正文块的技能；部分失败时不会把缺失文件或消失的目录条目列为已加载。全部失败仍不生成技能信封。

插件技能的 PluginUsed.success 表示正文是否成功展开，按每个引用分别记录；不代表模型任务执行成功。interjection 继续只记录派发，不新增 turn 归属的插件使用事件。

技能展开按解析时选定的路径、限定名称及插件身份查找条目；共用文件的原生/插件技能不会互相替代正文快照或插件变量。选定来源消失时该引用加载失败。

技能正文块、引用索引与预加载消息的 XML 属性会转义引号、尖括号及 &；正文 Markdown 保持原样，参数替换和引用去重仍基于原始文本。
