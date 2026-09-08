# client-surfaces Specification

## Purpose
定义应用入口、机器可读输出与会话存储之间的职责。覆盖 CLI 路由、headless 的输出格式，以及展示更新和持久化事实的分离；具体终端布局和全部命令参数尚未在此穷举。

## Requirements

### Requirement: Shared application entry
grow CLI SHALL 根据命令和参数路由到交互界面、Agent 服务或 headless 执行入口。

#### Scenario: 无交互运行
- **WHEN** 用户提供 headless prompt 参数
- **THEN** 构造 HeadlessOptions 并进入 headless 路径；CLI 管理命令由各自分支处理。

证据：`crates/codegen/cli/src/main.rs` — `HeadlessOptions`。

### Requirement: Machine readable headless output
headless SHALL 按 OutputFormat 区分 JSON、StreamingJson 与 StreamingMessagesJson 输出。

#### Scenario: 流式机器读取
- **WHEN** 输出格式选择 streaming messages JSON
- **THEN** 通过对应事件封装输出；include_partial_messages 控制该格式的部分消息事件。

证据：`crates/codegen/pager/src/headless.rs` — `HeadlessEmitter`。

### Requirement: Session storage separation
会话存储 SHALL 区分 summary.json、updates.jsonl、timeline.jsonl 与 sidebands。

#### Scenario: 恢复本地会话
- **WHEN** 读取会话目录
- **THEN** 使用对应存储适配器读取摘要、展示更新、Timeline 及旁路账本，展示更新不替代 Timeline 事实。

证据：`crates/codegen/shell/src/session/storage/mod.rs` — `TIMELINE_FILE`。

### Requirement: Update version probe process lifetime
自更新的目标版本探测和候选二进制冒烟检查 SHALL 将直接校验子进程的生命周期绑定到本次等待；超时或取消等待后 SHALL 请求终止该直接子进程，避免校验失败后继续后台运行。

#### Scenario: 校验超时
- **WHEN** `--version` 进程未在既定超时内退出
- **THEN** 校验返回失败，直接校验子进程被终止。

#### Scenario: 等待被取消
- **WHEN** 调用方取消正在运行的版本校验 future
- **THEN** 已启动的直接校验子进程被终止。

#### Scenario: 正常返回
- **WHEN** 版本进程正常退出并返回有效版本
- **THEN** 版本探测返回解析后的版本，冒烟检查返回成功。

证据入口：`crates/codegen/update/src/auto_update.rs` — `probe_version_by_exec`、`smoke_test_binary`。此要求不包含刻意脱离父进程的后台更新任务，也不承诺终止任意派生后代树。

### Requirement: Bounded post-update completion generation
自更新 SHALL 对每个 shell 的补全生成设置 10 秒子进程等待上限；超时或取消时请求终止直接子进程。补全生成命令失败 SHALL 不阻止后续补全步骤，也不覆盖已有补全文件。

#### Scenario: 补全进程超时
- **WHEN** 单个补全命令在 10 秒内未退出
- **THEN** 终止该直接子进程，保留目标文件并允许继续下一 shell。

#### Scenario: 更新等待取消
- **WHEN** 补全子进程运行期间调用方取消等待
- **THEN** 已启动的直接子进程被终止，已有补全文件保持不变。

#### Scenario: 成功与失败输出
- **WHEN** 补全进程结束
- **THEN** 只在成功且输出非空时更新目标文件；失败或空输出保留原文件。

证据入口：`crates/codegen/update/src/auto_update.rs` — `regenerate_completions`。不承诺任意派生后代树和文件系统调用的时限。

### Requirement: Download cleanup requires known stale age
自更新清理下载目录时 SHALL 只删除已确认文件年龄超过清理时限、且满足既有保留策略的旧版本候选；未来时间戳和无法读取的年龄 SHALL 不构成删除依据。

#### Scenario: 系统时钟回拨
- **WHEN** 可清理候选的修改时间晚于当前时间
- **THEN** 保留该文件，其他明确过期候选仍按保留策略清理。

#### Scenario: 正常旧版本清理
- **WHEN** 候选年龄明确超过清理时限
- **THEN** 删除不属于保留集合的旧版本，当前版本、保留的上一版本及新文件保持不变。

证据入口：`crates/codegen/update/src/auto_update.rs` — `cleanup_old_downloads`。不承诺文件系统竞争下检查与删除的原子性。

### Requirement: Download retention groups platform artifacts by version
下载目录清理 SHALL 保留当前版本及当前版本之外最高版本的全部已识别平台产物。保留结果 SHALL 不依赖文件枚举顺序，其他候选仍受既有年龄保护。

#### Scenario: 多平台上一版本
- **WHEN** 当前版本与最高其他版本各有多个平台产物
- **THEN** 两个版本的全部产物均保留，明确过期的更旧版本产物可删除。

#### Scenario: 回退安装
- **WHEN** 安装版本低于下载目录中的其他版本
- **THEN** 仍保留当前版本及最高其他版本的全部平台产物，不将其他版本选择改成只允许低于当前版本。

证据入口：`crates/codegen/update/src/auto_update.rs` — `cleanup_old_downloads`。不是每个平台单独选择一个不同的保留版本。

### Requirement: LSP inspection preserves permitted fallback visibility
LSP 配置诊断 SHALL 使用报告的项目信任结果展示允许来源合并结果，并在项目未信任时另列被禁用的项目定义。同名允许项 SHALL 不被禁用项目项遮蔽。

#### Scenario: 未信任项目与插件同名
- **WHEN** 项目定义与允许插件来源同名且项目未信任
- **THEN** JSON 和终端条目包含允许来源及明确标记 untrusted 的项目来源，允许项在同名禁用项之前。

#### Scenario: 项目已信任
- **WHEN** 项目已获信任且存在同名覆盖
- **THEN** 诊断显示项目覆盖结果，不额外重复列出已启用的低优先级同名配置；禁用插件定义以禁用标记单列。

证据入口：`crates/codegen/shell/src/inspect/mod.rs` — `list_lsp_servers`、`LspServerEntry`。此视图不宣称列出正在运行的服务器。

### Requirement: LSP inspection distinguishes plugin activation
LSP 诊断 SHALL 从插件注册表取得启用与信任状态；只有已启用且已信任插件参与允许来源合并，其他插件的声明作为独立诊断项展示。禁用和未信任 SHALL 分别以 disabled、untrusted 表达，终端与 JSON 使用同一状态。

#### Scenario: 禁用插件与活动插件同名
- **WHEN** 已信任但禁用插件与已启用插件定义同名服务器
- **THEN** 允许来源仍来自活动插件，禁用声明另列并标记 disabled，不遮蔽允许来源。

#### Scenario: 插件不受信任
- **WHEN** 插件已启用但未受信任
- **THEN** 其声明仅以 untrusted 诊断项展示，不进入允许来源合并。

证据入口：`crates/codegen/shell/src/inspect/mod.rs`、`crates/codegen/tools/src/implementations/lsp/config.rs`。展示不启动 LSP 进程。

### Requirement: Web fetch classifies HTML and PDF by media type
web_fetch SHALL 仅按 Content-Type 分号前的媒体类型判断 HTML/XHTML/PDF，忽略该部分大小写，参数和相似子类型 SHALL 不触发这些格式处理。

#### Scenario: 大写合法类型
- **WHEN** 响应为 TEXT/HTML、APPLICATION/XHTML+XML 或 APPLICATION/PDF
- **THEN** 分别按 HTML/XHTML 转换或 PDF 保存分支处理。

#### Scenario: 参数或相似类型
- **WHEN** 纯文本参数包含 text/html 或 application/pdf，或类型为 application/pdf-extra
- **THEN** 不因子串而进入 HTML/PDF 分支；纯文本内容保留，其他类型走既有分类。

### Requirement: Pager skill discovery delegates advertisement to reload
Pager 收到 Skills discovery 事件 SHALL 先维护目录监听，再请求技能重读；同一消费分支 SHALL 不因新目录注册额外请求独立命令发布，发布由重读完成路径负责。

#### Scenario: 新技能目录
- **WHEN** Pager 收到 Skills 事件并补挂新目录
- **THEN** 请求 ReloadSkills，不额外发送 AdvertiseCommands。

#### Scenario: 仅 workflow 修改
- **WHEN** Pager 收到 Workflows 事件
- **THEN** 维护监听后仍直接请求 AdvertiseCommands。

### Requirement: Failed screen mode persistence remains retryable
屏幕模式未配置时的保存失败 SHALL 恢复未配置状态，不把显示默认值记成已配置值。再次选择目标模式 SHALL 能重新发起保存。

#### Scenario: Failed initial fullscreen selection
- **WHEN** 原始 screen_mode 缺失，选择 Fullscreen 后保存失败
- **THEN** 回滚后仍为未配置，再次选择 Fullscreen 产生新的保存请求。

### Requirement: Sequential setting choices retain persistence ownership
同一普通设置的连续保存 SHALL 有确定顺序，较早失败 SHALL 不覆盖用户较新的待保存选择。所有待保存修改失败后 SHALL 恢复最后确认状态，而非未保存的中间值。

#### Scenario: Earlier failure with newer choice pending
- **WHEN** 同一设置已有更新选择，较早保存失败
- **THEN** 最新选择继续可见并继续保存。

#### Scenario: Consecutive failures
- **WHEN** 同一设置的连续修改均保存失败
- **THEN** UI 恢复到这些修改之前的最后确认状态。

#### Scenario: Latest queued choice and recursive reset
- **WHEN** 一个设置保存进行中，用户反复修改该设置，或通过递归 reset 动作修改
- **THEN** 每个 key 最多一个请求在途并保留最新排队选择，递归动作不重复登记；其他 key 可独立保存。

### Requirement: Default permission writes use ordered settings persistence
默认权限修改 SHALL 使用普通设置的顺序保存和回滚基线协调，且 SHALL 不改变当前会话权限或发送会话权限通知。

#### Scenario: Earlier default permission write fails
- **WHEN** 用户连续选择不同默认权限，较早保存失败
- **THEN** 最新选择继续可见并保存，连续失败回到最后确认默认值。

#### Scenario: Active session permission remains separate
- **WHEN** 默认权限保存成功或失败
- **THEN** 当前会话权限不变，未来会话仍按最终保存的默认配置初始化。

### Requirement: CLI clipboard export reports actual delivery
CLI export --clipboard SHALL 消费剪贴板实际结果，失败时返回错误，不得无条件报告已复制。反馈 SHALL 保留目标后端及是否确认的语义。

#### Scenario: Clipboard backends fail
- **WHEN** 剪贴板结果为 Failed
- **THEN** 导出返回错误，不能打印成功信息并正常退出。

#### Scenario: Delivery is unverified
- **WHEN** 后端只能确认发送而不能确认到达
- **THEN** 保留未确认发送的反馈，不升级为已复制。

#### Scenario: Confirmed delivery
- **WHEN** 后端确认接收
- **THEN** 使用该后端反馈，并按共享统计规则显示文本量。

### Requirement: Interactive session export resolves paths against session cwd
交互式 /export 的相对文件路径 SHALL 在展开 ~ 后按活动会话 cwd 解析，与会话路径补全的目录基准一致。

#### Scenario: Session cwd differs from process cwd
- **WHEN** 用户提供相对导出路径且会话目录不同于进程启动目录
- **THEN** 文件写入会话目录下对应位置。

#### Scenario: Absolute export path
- **WHEN** 展开后的导出目标是绝对路径
- **THEN** 保留该绝对目标，不附加会话目录。

### Requirement: Clipboard statistics count Unicode characters
共享剪贴板反馈的字符数 SHALL 按 Unicode scalar value 计算，不得将 UTF-8 字节数标为字符数；字符与行数单位 SHALL 使用正确单复数。

#### Scenario: Multibyte text
- **WHEN** 复制文本包含中文或 emoji
- **THEN** 每个 Unicode 标量计为一个字符，不按编码字节数计数。

#### Scenario: Empty and multiline text
- **WHEN** 文本为空或包含换行、组合字符
- **THEN** 字符计数覆盖全部 Unicode 标量，行数仍按 str::lines，空文本为零字符零行。

### Requirement: Transcript file export commits completed content atomically
CLI/TUI 文件导出 SHALL 先完整写入同目录临时文件并同步，再原子替换普通目标；提交前失败 SHALL 保留旧内容并清理临时文件。

#### Scenario: Partial temporary write fails
- **WHEN** 临时文件已写入部分内容后发生错误
- **THEN** 导出失败，原目标内容保持不变且临时文件清理。

#### Scenario: Existing symbolic link
- **WHEN** 目标为指向现存普通文件的符号链接
- **THEN** 提交替换链接目标，保留符号链接；悬空链接报错。

#### Scenario: Permissions and special targets
- **WHEN** 目标为已有普通文件、新文件或非普通文件
- **THEN** 已有权限保留且只读目标拒绝；Unix 新文件默认私有权限；非普通目标拒绝。

### Requirement: Interactive export waits for history replay completion
交互式完整会话导出 SHALL 在历史回放未完成时拒绝输出，并提示用户稍后重试，避免将部分历史作为完整导出。

#### Scenario: History still loading
- **WHEN** session.loading_replay 为 true 且已有部分可见消息
- **THEN** 导出只反馈加载中，不写文件或剪贴板。

#### Scenario: History loaded
- **WHEN** 历史回放完成并清除 loading_replay
- **THEN** 恢复正常导出，包含已加载的全部可导出消息。

### Requirement: Assistant copy file uses session cwd
交互式 /copy 的显式相对文件路径 SHALL 在展开 ~ 后按活动会话 cwd 解析，绝对路径保持原目标。

#### Scenario: Relative copy destination
- **WHEN** 会话目录不同于进程目录且提供相对复制文件路径
- **THEN** 文件写入会话目录，内容为选定 assistant 消息。

#### Scenario: Existing copy privacy
- **WHEN** 指定相对或绝对目标文件
- **THEN** 继续使用复制文件的私有权限策略，不因目录修复改用导出的权限继承规则。

### Requirement: Copy files commit private content atomically
显式复制文件和默认备份 SHALL 使用同目录临时文件完整写入并同步后原子提交；提交前失败 SHALL 保留旧内容并清理临时文件。Unix 临时文件及成功目标 SHALL 为 0600。

#### Scenario: Partial copy file write fails
- **WHEN** 临时文件部分写入后发生错误
- **THEN** 原目标内容和权限不变，临时文件清理。

#### Scenario: Previously public copy file
- **WHEN** 原目标为 0644 且新复制成功
- **THEN** 新目标包含完整文本且权限收紧为 0600。

#### Scenario: Symbolic link copy target
- **WHEN** 路径是现存普通文件的符号链接
- **THEN** 保留链接并更新其目标；悬空链接、非普通或只读目标报错。

### Requirement: Assistant copy formats only the selected message
/copy N SHALL 从最新 assistant 消息倒序选择，找到目标后停止扫描并只格式化目标消息，不为选择创建所有历史正文副本。

#### Scenario: Selected message among mixed blocks
- **WHEN** scrollback 混有用户和系统块，N 是有效 assistant 序号
- **THEN** 只按 assistant 消息倒序计数并复制对应内容。

#### Scenario: Invalid copy index
- **WHEN** Action 的 N 为零或超过 assistant 消息数量
- **THEN** 返回明确提示，不 panic 且不输出文件或剪贴板。

### Requirement: Copy notices preserve delivery evidence
交互式 /copy 和 /export 的持久通知 SHALL 保留实际后端及确认程度，不得将未确认发送描述为已到达剪贴板。

#### Scenario: Unverified send with backup
- **WHEN** 复制结果为未确认发送且存在备份文件
- **THEN** 通知保留发送语义并显示备份位置，不宣称已复制到剪贴板。

#### Scenario: Confirmed backend and failures
- **WHEN** 后端已确认、仅文件成功或全部失败
- **THEN** 分别保留该后端反馈、文件回退位置或失败提示，成功备份位置仍可见。

### Requirement: Copy rejects invalid numeric indices instead of writing files
/copy 的首个参数若为带单个可选正负号的 ASCII 整数字面量，SHALL 作为序号解释；零、负数或超范围值 SHALL 返回错误，不得回退为文件路径。

#### Scenario: Invalid integer with optional destination
- **WHEN** 输入为负数、零或超范围整数字面量，可能带后续文件参数
- **THEN** 解析返回错误，不创建复制文件 Action。

#### Scenario: Explicit numeric filename
- **WHEN** 文件名通过 ./ 前缀或扩展名明确为路径
- **THEN** 继续按文件目标解析，合法正整数序号也保持原语义。

### Requirement: Explicit transcript file writes run off the UI thread
交互式 /copy 和 /export 的显式文件输出 SHALL 在快照内容与目标后，通过后台任务执行文件 I/O，不在 UI dispatcher 中执行写入或同步。

#### Scenario: Slow file write
- **WHEN** 显式文件输出的底层写入尚未完成
- **THEN** dispatcher 已返回，成功反馈等待实际提交结果。

### Requirement: Transcript file jobs preserve order and origin
同一 Pager 应用的显式会话文件任务 SHALL 串行按提交顺序执行，待处理请求数量 SHALL 有限；结果反馈 SHALL 绑定原 agent/session。

#### Scenario: Repeated destination
- **WHEN** 前一文件任务未完成时再次提交同一路径
- **THEN** 后一任务等待前一任务完成，最终内容按提交顺序确定。

#### Scenario: Origin no longer matches
- **WHEN** 完成时原视图已移除或绑定其他会话
- **THEN** 不给新会话发布旧任务反馈，但仍推进待处理队列。

#### Scenario: Failure or saturated queue
- **WHEN** 写入失败、后台任务异常或待处理队列已满
- **THEN** 明确报告对应失败；任务异常不阻塞后续已接纳请求，满载时不静默丢弃旧任务。

### Requirement: External pager transcripts have private owned temporary files
普通 Markdown 和 minimal ANSI 分页器会话临时文件 SHALL 从创建起采用 Unix 0600 权限，并将清理责任绑定到待处理请求和分页器调用的生命周期。

#### Scenario: Transcript creation or replacement
- **WHEN** 创建会话文件或替换尚未打开的分页器请求
- **THEN** 新文件包含完整正文，部分写入失败清理新临时文件，被替换请求的旧文件释放并删除。

#### Scenario: Pager request ends
- **WHEN** 分页器正常结束、挂起返回非重试错误或应用正常释放待处理请求
- **THEN** 释放拥有的临时文件并尝试删除，无需依赖成功路径的显式删除。

#### Scenario: Suspend retry
- **WHEN** 挂起超时且请求等待重试
- **THEN** 同一文件所有权随请求保留，重试期间正文仍可读取。

### Requirement: External pager failures remain visible after terminal restoration
外部分页器 SHALL 保留启动和退出结果，在终端恢复后通过当前屏幕模式可见的通知反馈失败。

#### Scenario: Pager cannot start
- **WHEN** 分页器可执行程序不存在或启动返回操作系统错误
- **THEN** 界面恢复后显示启动失败及原因，不把失败静默当作正常结束。

#### Scenario: Pager exits unsuccessfully
- **WHEN** 分页器返回非零退出状态或被信号终止
- **THEN** 界面恢复后显示失败状态，临时文件仍按请求所有权清理。

#### Scenario: Successful exit or pending handoff
- **WHEN** 分页器成功退出或终端挂起仍等待重试
- **THEN** 成功退出不新增失败提示；未启动的重试请求保持既有等待反馈，不报告进程失败。

### Requirement: External pager preserves quoted argument boundaries
PAGER SHALL 使用 shell 风格引号及转义进行参数分词，并直接调用可执行程序，不引入 shell 求值。

#### Scenario: Quoted executable and argument
- **WHEN** 程序路径或参数包含通过引号或转义表示的空格
- **THEN** 保留完整参数边界，追加的会话路径仍为独立参数。

#### Scenario: Invalid command
- **WHEN** 引号未闭合或解析后的程序名为空
- **THEN** 返回配置错误，通过已有分页器失败反馈显示，不尝试拆分后的其他程序。

#### Scenario: Literal syntax and less defaults
- **WHEN** 参数包含变量、命令替换或操作符文本，或程序是 less
- **THEN** 参数不进行 shell 求值；less 的既有 ANSI 与末尾定位参数保持，其他程序不添加这些参数。

### Requirement: Incremental transcripts restart after owner reload
Minimal 在途 transcript SHALL 在原 agent 重连时使旧分帧结果失效，等待 reload 结束后从最终正文重新构建，不输出暂存期间跳过条目形成的前缀。

#### Scenario: Reload spans a render frame
- **WHEN** 原 agent 正在 reload 且 transcript 尚未生成文件
- **THEN** 保留请求但不消费临时 staging 条目；期间新请求同样等待。

#### Scenario: Reload resolves between frames
- **WHEN** 完整 replay、cursor 成功或失败回滚在下次 pump 前完成
- **THEN** 旧前缀和 ID 失效，从最终正文的全部当前条目重新开始。

#### Scenario: Other view or agent changes
- **WHEN** 用户切换标签或其他 agent reload
- **THEN** 不改变原构建 owner，未 reload 的原构建继续推进。

### Requirement: Transcript viewing rejects incomplete initial history
首次会话历史加载未完成时，transcript SHALL 提示稍后重试，不将当前部分正文作为完整会话打开。

#### Scenario: Initial loading in any screen mode
- **WHEN** loading_replay 为 true 且不是 minimal 重连等待窗口
- **THEN** 不创建分页器文件或新分帧构建，显示加载提示。

#### Scenario: Initial loading completes
- **WHEN** 历史加载完成后再次请求 transcript
- **THEN** 按当前完整正文正常创建 Markdown 文件或 minimal 条目快照。

#### Scenario: Minimal reconnect remains pending
- **WHEN** minimal 原 agent 有活动 SessionReload
- **THEN** 保留既有等待和最终正文自动重建，不因首次加载保护丢弃请求。

### Requirement: Input diagnostics dumps are independent private files
每次 input recorder 导出 SHALL 创建独立文件，即使时间戳相同也不覆盖已有快照；Unix 从创建起 SHALL 使用 0600 权限。

#### Scenario: Repeated dump within one second
- **WHEN** 同一秒导出两份不同诊断快照
- **THEN** 返回两个不同路径，每份保持对应完整正文。

#### Scenario: Partial write or directory failure
- **WHEN** 写入、同步或创建目录失败
- **THEN** 返回错误，提交前新临时文件清理，已有快照保持不变。

### Requirement: Default scroll recordings use independent paths
默认滚动日志路径 SHALL 包含时间戳之外的独立标识，避免同秒创建的记录器复用默认文件。

#### Scenario: Same timestamp recorders
- **WHEN** 同一时间创建两个默认路径的记录器
- **THEN** 路径不同，两个记录器各自保留写入内容。

#### Scenario: Enabled recorder remains idle
- **WHEN** 生成默认路径并启用记录器但尚无记录
- **THEN** 不创建文件，保持首条记录触发创建的行为。

### Requirement: Scroll log status reflects recorder failure
滚动日志状态 SHALL 将已因 I/O 失败停用的记录器视为关闭，运行时切换 SHALL 可从此状态直接重新启用。

#### Scenario: Recorder fails
- **WHEN** 打开或写入失败使 recorder 进入 Disabled
- **THEN** 状态查询返回关闭，滚动输出不因诊断失败改变。

#### Scenario: Toggle after failure
- **WHEN** 失败后用户再次切换日志
- **THEN** 创建新的延迟打开记录器并返回新路径，无需先额外关闭一次；Pending/Open 仍正常切换为关闭。

### Requirement: Minimal mode renders the enabled FPS HUD
minimal 模式 SHALL 将已启用 FPS HUD 接入真实 draw hook 耗时采样，并在空间足够时展示现有统计。

#### Scenario: Enabled minimal frame
- **WHEN** HUD 开启且 live viewport 至少可容纳 HUD 两行和正文三行
- **THEN** 顶部显示 HUD，正文及光标布局使用剩余独立区域，不被覆盖。

#### Scenario: Disabled or tiny viewport
- **WHEN** HUD 关闭或空间不足
- **THEN** 不占用 HUD 行；关闭时不记录样本，输入区域优先保留。

#### Scenario: On-demand measurement
- **WHEN** minimal 已有绘制发生
- **THEN** 记录该绘制路径耗时，不增加空闲刷新循环，也不将后台 PTY 完成或屏幕刷新率宣称为采样结果。

### Requirement: External pager feedback preserves request origin
外部分页器请求 SHALL 在创建到挂起重试和完成之间保留原 agent/session 来源，失败和等待反馈不得写入后来切换到的其他会话正文。

#### Scenario: View changes before pager completion
- **WHEN** 原 root 或 child 会话请求分页器后用户切换视图，随后发生等待或失败
- **THEN** 会话通知绑定原匹配来源，其他会话正文不接收该通知。

#### Scenario: Origin is removed or rebound
- **WHEN** 反馈到达时原来源已移除或会话身份不再匹配
- **THEN** 不把通知写入新会话，通过应用级反馈保持失败可见。

#### Scenario: Suspend retry preserves the request
- **WHEN** 终端挂起超时并重试分页器
- **THEN** 文件所有权、ANSI 设置与原来源一同保留，重试不改用当前活动视图。

### Requirement: Clipboard hint metadata probes skip busy native readers
macOS 剪贴板提示元数据入口 SHALL 在原生粘贴读取占用串行锁时返回不可用，不等待该读取释放锁。

#### Scenario: Paste reader owns the native lock
- **WHEN** 图片提示尝试读取版本或类型，而原生剪贴板锁已被占用
- **THEN** 元数据探测立即跳过并返回未知，保留原生访问串行性。

### Requirement: Clipboard hint dedup commits classified versions only
图片提示轮询 SHALL 仅将具有有效分类版本的已处理结果用于去重，不将不可用分类视为确认无图片。

#### Scenario: Classification is temporarily unavailable
- **WHEN** cheap 探测看到新版本但分类返回未知
- **THEN** 不提交该版本，后续允许的轮询继续重试。

#### Scenario: Version changes between probe stages
- **WHEN** cheap 与非图片分类返回不同版本
- **THEN** 去重记录分类结果自身版本，不记录较早 cheap 版本。

### Requirement: Clipboard metadata snapshots reject observed version races
macOS 剪贴板元数据快照 SHALL 在类型分类前后读取版本，仅在版本一致且类型分类可用时返回有效版本与分类。

#### Scenario: Clipboard changes during classification
- **WHEN** 类型分类前后的版本不同
- **THEN** 返回未知结果，不将旧版本绑定到新类型，也不在该调用内循环重试。

#### Scenario: Types cannot be read
- **WHEN** 类型列表不可用
- **THEN** 返回未知，不能据此确认没有图片。

#### Scenario: Stable known metadata
- **WHEN** 前后版本一致且类型可用
- **THEN** 返回该版本及共享类型规则的图片分类，保持文件 URL 优先规则。

### Requirement: Clipboard image fallback owns isolated private files
macOS AppleScript 图片回退 SHALL 为每次调用分配独立私有临时目录，并持有到脚本执行和图片读取完成。

#### Scenario: Concurrent fallback requests
- **WHEN** 两个调用分别读取图片
- **THEN** 使用不同目录与文件路径，不能覆盖或清理另一调用的产物；Unix 目录权限为 0700。

#### Scenario: Fallback completes or fails
- **WHEN** 调用正常返回、无图片、读取失败或脚本返回错误
- **THEN** 释放该调用的目录所有者并尝试清理全部残留文件，不依赖仅成功分支删除选中文件。

### Requirement: Clipboard AppleScript paths are data arguments
macOS 剪贴板图片脚本 SHALL 通过 osascript 参数接收路径，不将路径插入 AppleScript 源码。

#### Scenario: Special characters in a path
- **WHEN** 图片或临时目录路径包含空格、双引号、反斜杠、换行或 Unicode
- **THEN** 路径作为一个参数保留，字符不改变脚本结构。

#### Scenario: Read and write image scripts
- **WHEN** 调用图片读取、附件读取或图片写入脚本
- **THEN** 统一使用参数边界传递路径，保持既有类型优先和临时文件生命周期。

### Requirement: Interactive doctor collection stays off the UI thread
TUI doctor 报告及修复前采集 SHALL 在受控后台执行，不在 dispatcher 等待 tmux 子进程或文件扫描。

#### Scenario: A diagnostic probe is slow
- **WHEN** 用户请求报告、修复列表或具体修复，某个探测尚未完成
- **THEN** dispatcher 已返回，UI 可继续处理输入，采集并发保持有限。

#### Scenario: Diagnostic origin changes
- **WHEN** 采集完成时原会话已移除、被替换或重新绑定到其他会话
- **THEN** 不将旧报告或修复预览提交到新会话。

#### Scenario: Read-only report and explicit fix
- **WHEN** 报告或修复采集完成
- **THEN** 纯报告不修改配置，具体修复继续经过原有预览和确认流程。

### Requirement: Tmux diagnostic output is bounded
共享 tmux 查询 SHALL 对 stdout 和 stderr 分别限制64 KiB，不将超限截断结果作为有效诊断值。

#### Scenario: A pipe exceeds its limit
- **WHEN** 任一管道超过64 KiB
- **THEN** 返回输出超限错误，等待中的进程树被终止并回收，不继续无限收集。

#### Scenario: Output fits the limit
- **WHEN** 输出不超过每流限额且进程在期限内完成
- **THEN** 保留全部输出并按原有规则解析，退出后管道清理窗口保持。

### Requirement: Doctor SSH fixes honor shell config directories
SSH自动修复 SHALL 使用请求中可见的ZDOTDIR选择zsh配置，使用非空XDG_CONFIG_HOME选择fish配置；预览、写入和后置检查 SHALL 使用同一目标。未设置覆盖时保持默认。

#### Scenario: Custom shell config directory
- **WHEN** 当前shell为zsh且设置安全绝对ZDOTDIR，或为fish且设置安全绝对非空XDG_CONFIG_HOME
- **THEN** 分别使用该目录下.zshrc或fish/config.fish，不写入HOME默认路径。

#### Scenario: Unsafe relevant override
- **WHEN** 相关覆盖不满足现有安全绝对目录约束
- **THEN** 规划明确拒绝，不悄悄改写默认文件。

#### Scenario: Default or irrelevant override
- **WHEN** 覆盖未设置、fish的XDG为空，或变量不适用于当前shell
- **THEN** 按当前shell默认路径或其相关覆盖规划，无关变量不改变目标。

### Requirement: Tmux reload guidance distinguishes reattachment
诊断与修复提示 SHALL 要求显式重新加载配置以应用到运行中的tmux服务器，不将客户端detach/reattach描述为配置重载的替代方式。Grow SHALL 保持不自动执行重载。

#### Scenario: Persistent tmux fix completed
- **WHEN** 显示tmux修复预览、结果或诊断建议
- **THEN** 指向source-file重载，必要的客户端重连作为重载后的独立步骤。

#### Scenario: Path cannot be safely shown as a command
- **WHEN** 文件路径不能安全显示为shell命令
- **THEN** 提示手动加载变更文件，不宣称重新连接即可激活配置。

### Requirement: Standalone doctor checks the selected shell target
独立doctor报告的SSH已配置判定 SHALL 复用修复规划使用的shell配置目录解析。

#### Scenario: Custom target configured
- **WHEN** 自定义zsh/fish配置目标中已包含有效托管alias
- **THEN** 报告按该目标判定，不因HOME默认文件缺失而重复报错。

#### Scenario: Only the inactive default target is configured
- **WHEN** 显式配置目录指向的文件未配置，但HOME默认文件有alias
- **THEN** 不把默认文件状态冒充实际目标的已配置状态。

### Requirement: Tmux fix targets have explicit provenance
tmux自动修复 SHALL 将选择后的配置目标固定用于诊断、预览、写入和重载提示；无法确定目标时 SHALL 提供显式路径选择方式，不静默改写猜测的默认文件。

#### Scenario: Explicit custom target
- **WHEN** 用户为tmux修复指定合法配置目标
- **THEN** 所有阶段使用该目标，保持已有确认与事务检查。

#### Scenario: Ambiguous server configuration evidence
- **WHEN** 配置来源查询不可用、为空或无法无歧义确定目标
- **THEN** 说明无法自动确定并要求显式目标，不把逗号列表任一项或HOME默认文件当成已证实来源。

#### Scenario: Consistent Byobu target
- **WHEN** Byobu修复选择有效自定义配置目录
- **THEN** 诊断与写入使用同一目标，不再显示另一个固定默认路径。

### Requirement: Doctor surfaces share configured SSH status
CLI与TUI doctor报告 SHALL 使用同一当前shell配置目标判定SSH托管alias状态。

#### Scenario: Local managed alias exists
- **WHEN** 本地当前shell目标含有效托管SSH alias
- **THEN** 两种报告均移除重复的SSH配置建议，不把持久化状态当作实时路由探测。

#### Scenario: Target not configured or session remote
- **WHEN** 当前目标未配置，或报告来自SSH/VS Code Remote
- **THEN** 不用其他目标或远端shell配置隐藏本地SSH配置建议。

### Requirement: Clipboard image scripts have bounded process lifetimes
macOS附件和图片AppleScript调用 SHALL 限制执行为5秒、stdout和stderr各1MiB，超限不得作为有效结果。已有进程组 SHALL 在调用结束时回收，输出收尾 SHALL 有期限。

#### Scenario: Script hangs or floods output
- **WHEN** 图片或附件脚本超过执行时限或任一输出限额
- **THEN** 返回错误并终止进程组，不继续无限收集输出。

#### Scenario: Leader exits with descendants holding pipes
- **WHEN** 脚本leader退出但后代仍持有管道
- **THEN** 回收拥有的进程组，管道收尾最多额外等待300ms。

#### Scenario: Normal script result
- **WHEN** 脚本在预算内结束
- **THEN** 保留完整stdout/stderr和退出状态，既有结果解析与错误报告继续有效。

#### Scenario: Process group cleanup fails
- **WHEN** 操作系统拒绝进程组清理
- **THEN** 返回清理错误，不宣称成功；已有执行失败原因同时保留。

### Requirement: macOS clipboard encoded images have a read budget
macOS剪贴板图片读取 SHALL 限制编码数据为50,000,000字节；空结果保持无图片语义，超限返回错误。

#### Scenario: Native image exceeds the budget
- **WHEN** NSData报告长度超过预算
- **THEN** 在分配Rust图片buffer和复制前拒绝，不把超限当作不可用转入脚本回退。

#### Scenario: Fallback image file exceeds the budget
- **WHEN** 脚本回退图片文件读取超出预算
- **THEN** 实际最多读取预算加1字节后报错，临时目录按既有所有权释放。

#### Scenario: Image fits the budget
- **WHEN** 编码数据非空且不超过预算
- **THEN** 保留全部数据与既有MIME/图片类型优先级。

### Requirement: Clipboard RGBA encoding validates input shape
剪贴板RGBA编码 SHALL 在编码和输出分配前验证非零宽高、u32尺寸可表示性、宽高乘4可表示性及精确字节长度；无效输入返回错误，不因尺寸截断、整数溢出或编码器长度断言panic。

#### Scenario: Invalid dimensions or byte count
- **WHEN** 宽高为零、尺寸或长度计算不可表示，或buffer过短/过长
- **THEN** 返回明确错误，不调用PNG编码器。

#### Scenario: Valid RGBA input
- **WHEN** 尺寸有效且buffer字节数恰好为宽乘高乘4
- **THEN** 保持PNG编码并保留像素内容。

### Requirement: Placeholder image caps bound actual reads
占位图片文件加载 SHALL 对实际读取执行预算加1字节上限，而非仅在完整读取后检查；现有授权、扩展名和MIME验证继续适用。

#### Scenario: File grows after size inspection
- **WHEN** 前置文件大小检查后内容超出预算
- **THEN** 最多读取预算加1字节并返回TooLarge，不继续读取完整内容。

#### Scenario: Exact budget and read failure
- **WHEN** 图片恰好在预算内或读取发生I/O错误
- **THEN** 分别保留完整有效图片或返回既有ReadFailed分类。

#### Scenario: Reporting observed excess
- **WHEN** 有界读取发现超限
- **THEN** 错误中的actual表示已观察字节，不宣称它是完整文件长度。

### Requirement: Orphan image reads honor remaining recovery budget
占位图片恢复 SHALL 在读取前使用单图上限与本次恢复剩余总预算的较小值作为读取上限；预算耗尽时停止恢复。

#### Scenario: Remaining budget is tighter
- **WHEN** 剩余总预算小于单图上限且候选文件超过该余额
- **THEN** 有界拒绝并停止后续恢复，保留已恢复图片，不为先验证MIME而完整读取超额候选。

#### Scenario: Per-image cap is the limiting factor
- **WHEN** 单图上限小于或等于剩余额度且候选超过单图上限
- **THEN** 该候选失败后仍允许后续合法更小图片恢复。

#### Scenario: Exact remaining budget
- **WHEN** 有效图片大小等于剩余额度
- **THEN** 完整恢复该图片，并在下一候选前停止；已有附图及图片编号保持。

### Requirement: Image file URIs preserve literal path bytes
图片附件file URI SHALL 通过标准file-path URL转换生成与解析，百分号只解码一次；不在literal路径和解码路径之间猜测。

#### Scenario: Literal percent and reserved characters
- **WHEN** 图片文件名含字面%20、%2F、空格、#、?或Unicode
- **THEN** URI往返保留同一路径，去重不把不同文件合并。

#### Scenario: URI is not a plain local file identity
- **WHEN** 输入非file URI，或带query/fragment
- **THEN** 不将其当成本地图片规范路径用于去重。

### Requirement: Dropped image reads have an encoded byte budget
拖拽及路径粘贴图片入口 SHALL 在图片识别前限制编码数据为50,000,000字节，实际最多读取预算加1字节；打开的来源必须是普通文件。

#### Scenario: Oversized or growing image file
- **WHEN** 文件长度或实际读取超过预算
- **THEN** 不生成图片附件，按既有规则保留普通文件路径回退。

#### Scenario: Valid image within budget
- **WHEN** 普通图片文件非空且未超限
- **THEN** 保留原有图片识别、维度和数据；合法符号链接拖入语义保持。

### Requirement: Drop batches bound retained image bytes
The shared drop classifier SHALL retain at most 50,000,000 encoded image bytes per paste. The existing per-file read limit SHALL continue to bound the candidate being examined before aggregate admission.

#### Scenario: Batch exceeds the retained budget
- **WHEN** another valid image would exceed the per-paste retained byte allowance
- **THEN** return no classified entries for the whole paste, allowing existing text fallback rather than delivering a partial batch.

#### Scenario: Exact budget with ordinary paths
- **WHEN** image bytes exactly fill the allowance and other entries are ordinary paths
- **THEN** preserve all entries in source order, without charging path text as image bytes.

### Requirement: Dashboard preserves mixed drop paths
Dashboard dispatch and peek SHALL insert recognized image and ordinary-file drop entries in source order for bracketed paste, paste-key text, and deferred file URL completion.

#### Scenario: Image and ordinary file share a paste
- **WHEN** a recognized batch contains an image and an ordinary file path
- **THEN** attach the image and insert the ordinary path as text in the same target, without dropping that path through image-only filtering.

#### Scenario: Image cap does not suppress file text
- **WHEN** the target rejects an image because its attachment cap is reached and the batch also contains an ordinary path
- **THEN** keep the existing image rejection feedback and still insert the ordinary path.

#### Scenario: Explicit file URL cannot be loaded as an image
- **WHEN** the shared classifier recognizes an explicit file URL as an ordinary path, including a missing image file
- **THEN** insert that path as text and report successful handling without adding an image attachment.

#### Scenario: Question and stale target guards
- **WHEN** a peek is in question mode or a deferred target has become stale
- **THEN** preserve existing text-only admission and deferred discard rules rather than routing mixed entries into the hidden reply.

### Requirement: Deferred file URLs survive classification miss
Agent and Dashboard clipboard completion SHALL preserve non-empty unclassified file URLs as text after a successful no-raster probe when the source has no non-whitespace original text.

#### Scenario: No original text and no classified attachments
- **WHEN** a successful file URL probe yields no classified entries and original text is missing, whitespace-only or unreadable
- **THEN** insert the raw URL text once and report its insertion result instead of treating the clipboard as empty.

#### Scenario: Original text or classified entries exist
- **WHEN** original non-whitespace text exists or file classification already produced a handled/rejected result
- **THEN** keep existing insertion behavior without additionally inserting raw URL fallback text.

#### Scenario: Probe or target is not admissible
- **WHEN** the attachment probe failed, was dropped, persistence failed, or existing question/stale target guards reject the completion
- **THEN** do not insert URL fallback text.

### Requirement: Kitty image conversion bounds source pixels
Grow SHALL require nonzero dimensions and at most 16,000,000 source pixels before converting non-PNG image bytes for Kitty overlays, including the macOS sips backend.

#### Scenario: Conversion source exceeds pixel budget
- **WHEN** a non-PNG source header exceeds the pixel allowance or has invalid dimensions
- **THEN** return no converted preview bytes before invoking a conversion backend, preserving the original attachment bytes.

#### Scenario: Valid conversion and direct transmission
- **WHEN** source dimensions fit the conversion budget or the protocol directly transmits encoded data
- **THEN** retain existing backend selection and direct-transmission behavior; this limit does not claim to bound terminal-side decode memory.

### Requirement: Sips conversion owns private temporary files
Sips conversion SHALL keep source and output files within a unique owned temporary directory with Unix mode 0700, and release that directory on ordinary success and failure exits.

#### Scenario: Conversion fails before or after process creation
- **WHEN** source creation/write, command launch, conversion status or output retrieval fails
- **THEN** release the owned directory and its files rather than relying on reaching per-file cleanup statements.

#### Scenario: Independent successful conversions
- **WHEN** separate conversions create workspaces and produce output
- **THEN** use distinct private directories, return the output bytes and release each workspace independently.

### Requirement: Sips converter has an owned execution deadline
The spawned sips converter SHALL have a 10-second execution deadline and an independently owned process group. Ordinary completion, timeout and wait failure SHALL attempt group cleanup before returning to conversion-file cleanup.

#### Scenario: Converter stalls
- **WHEN** the spawned converter does not exit within its execution allowance
- **THEN** report timeout, kill the owned group and reap the direct child before releasing its conversion scope.

#### Scenario: Leader exits with descendants
- **WHEN** the converter leader exits but members of its owned group remain
- **THEN** terminate the remaining group before returning its exit status.

#### Scenario: Cleanup fails
- **WHEN** process cleanup fails for a reason other than an already-gone Unix group
- **THEN** report the cleanup failure rather than presenting conversion as successfully cleaned up.

### Requirement: Sips output reads have an encoded budget
Sips output loading SHALL accept only non-empty regular-file data of at most100,000,000 bytes and consume at most that allowance plus1 byte. Unix output opening SHALL reject symlinks and avoid blocking on FIFOs.

#### Scenario: Empty oversized or unreadable output
- **WHEN** output is empty, too large or unreadable
- **THEN** reject the sips result, release its owned temporary directory and retain existing conversion fallback behavior.

#### Scenario: Exact budget and growth after metadata
- **WHEN** actual bytes exactly fit the allowance or exceed it after metadata checking
- **THEN** preserve exact-fit data, or reject after consuming at most allowance plus1, respectively.

### Requirement: Sips process failures identify their stage
Sips process errors SHALL identify startup, group attachment, wait or cleanup stages while retaining the original error kind and diagnostic text.

#### Scenario: Process startup fails
- **WHEN** spawning or its detach/pre-exec hook fails
- **THEN** report startup-stage context with the underlying error rather than an unattributed OS error.

#### Scenario: Process ownership or cleanup fails
- **WHEN** attachment, waiting or cleanup reports an error
- **THEN** include the failing stage without suppressing errors or changing existing recovery behavior.

### Requirement: Prompt image viewer loading is deferred
Opening an image viewer from a prompt image SHALL return a loading viewer without performing image-file reads or image conversion on the input thread, and use the existing background completion pipeline for both encoded-memory and durable-file sources.

#### Scenario: Real prompt image interaction
- **WHEN** the user opens an image chip that has memory bytes or a durable source path
- **THEN** preserve its display number and enqueue owned background loading using the requesting terminal protocol.

#### Scenario: Viewer closes or reopens before completion
- **WHEN** a background result belongs to an older opening, even of the same attachment
- **THEN** discard it through owner/target validation instead of replacing the current viewer.

#### Scenario: Background source fails
- **WHEN** source reading or decoding fails
- **THEN** settle the current loading viewer through existing failure handling without blocking input on that work.

### Requirement: Background image viewer source bytes are bounded
The background image viewer loader SHALL admit only non-empty sources up to50,000,000 encoded bytes, checking memory length before copying and consuming at most allowance plus1 byte from files.

#### Scenario: Source exceeds allowance
- **WHEN** memory length, file metadata or actual file bytes exceed the allowance
- **THEN** return a failed load before decoding/conversion, preserving existing viewer failure completion handling.

#### Scenario: Valid file or memory source
- **WHEN** source bytes exactly fit the allowance and contain a valid image
- **THEN** retain source data and existing protocol conversion behavior for both representations, including valid symlink file sources.

#### Scenario: Special file or growing source
- **WHEN** an opened source is not a regular file or its content grows beyond the allowance
- **THEN** reject it; Unix FIFO opening must not block, and actual reading stops at allowance plus1.

### Requirement: Slash MRU writes own unique temporary files
Each slash MRU snapshot write SHALL own a unique temporary file in the destination directory and publish a complete snapshot by atomic replacement. Failure cleanup SHALL not remove another writer's temporary path.

#### Scenario: Concurrent snapshot writers
- **WHEN** independent writers publish snapshots to the same MRU destination
- **THEN** each write uses its own temporary file and the resulting file contains one complete snapshot, with existing last-writer-wins semantics.

#### Scenario: Legacy temporary path exists or publication fails
- **WHEN** the old fixed temporary path already exists or destination replacement fails
- **THEN** do not overwrite/remove the unrelated fixed path, and clean only the failing writer's owned temporary file.

### Requirement: Slash MRU loading bounds encoded input
Slash MRU loading SHALL accept only regular files with at most 1,048,576 encoded bytes, check metadata on the opened handle, and read at most the allowance plus one byte. Rejected reads SHALL complete initialization with persistence disabled for that store, without overwriting the source.

#### Scenario: File exceeds allowance or grows during reading
- **WHEN** the opened file is too large or a bounded read observes more than the allowance
- **THEN** loading rejects the bytes before JSON parsing and subsequent command use does not produce a persistence snapshot

#### Scenario: Special file on Unix
- **WHEN** the MRU path points to a FIFO without a writer or another non-regular file
- **THEN** loading rejects it without waiting for a FIFO writer and disables persistence

#### Scenario: Normal and missing stores
- **WHEN** a regular file, including a symbolic-link target, contains valid JSON within the allowance
- **THEN** its entries load with the existing 256-entry retention policy
- **WHEN** the path is absent
- **THEN** an empty initialized store remains eligible for persistence

### Requirement: Slash MRU background writes coalesce pending snapshots
Slash MRU persistence SHALL retain at most one pending complete snapshot in addition to a snapshot being written. New submissions SHALL replace the pending snapshot, and queueing SHALL not wait for disk IO or notification capacity. An unavailable writer SHALL return failure to the controller for its existing dirty-state retry instead of writing synchronously.

#### Scenario: Slow writer receives multiple updates
- **WHEN** multiple complete snapshots arrive before the worker takes pending work or while it writes a previous snapshot
- **THEN** only the latest pending snapshot remains and the worker can publish it after the current write finishes

#### Scenario: Writer unavailable
- **WHEN** the background writer cannot start or its notification receiver is disconnected
- **THEN** persistence reports failure without writing the file on the submitting thread and the controller retains dirty state for a future command

### Requirement: History search submission does not wait for worker capacity
History search SHALL retain a coalesced latest pending request and submit updates without waiting for worker queue capacity. Closing its daemon SHALL not wait for notification capacity or ongoing matching.

#### Scenario: Worker is busy while inputs accumulate
- **WHEN** item refreshes and queries arrive before the worker can process pending work
- **THEN** submissions return without waiting for channel capacity and the next request retains the latest item refresh with its latest following query

#### Scenario: Closing with pending work
- **WHEN** the history search daemon is dropped while work is pending or being matched
- **THEN** stop takes precedence over pending work without blocking the UI on channel capacity

### Requirement: History search accepts only current request results
History search SHALL identify results by the originating request and expose only results matching the latest submitted request. Submitting a query or item refresh SHALL invalidate previously selectable results immediately.

#### Scenario: Reopen or change query before completion
- **WHEN** an older request completes after reopening or submitting a newer query
- **THEN** its results cannot be displayed or selected, while the current request result can be applied once

#### Scenario: Item refresh retains navigation intent
- **WHEN** items refresh and the same query is resubmitted after manual navigation
- **THEN** stale results are hidden while waiting and the retained selection index is clamped to current results without resetting the existing navigation intent

### Requirement: Local draft recovery bounds source reads
Local draft recovery SHALL accept only regular file sources, read at most 262,145 bytes, and reject actual content above 262,144 bytes before JSON parsing. Oversized regular files SHALL follow the existing quarantine policy.

#### Scenario: File grows after metadata observation
- **WHEN** a draft grows beyond the byte allowance after opening and metadata inspection
- **THEN** bounded reading detects the extra byte and quarantines the draft without restoring its content

#### Scenario: Special file source
- **WHEN** the draft path opens a non-regular file, including a Unix FIFO without a writer
- **THEN** loading returns an error without waiting for a FIFO writer or moving the special file into quarantine

#### Scenario: Regular file at allowance
- **WHEN** valid draft JSON including trailing whitespace exactly fits the allowance, including through a regular-file symlink
- **THEN** recovery preserves the existing record validation and restoration behavior

### Requirement: Local draft records contain at most one prompt source
Local draft validation SHALL reject records containing both composer and staged_prompt, including empty values. Invalid writes SHALL not replace or remove an existing record; invalid loaded records SHALL follow quarantine policy and SHALL not restore either prompt or its deferred behavior.

#### Scenario: Two prompt sources on disk
- **WHEN** a record contains both composer and staged_prompt
- **THEN** loading quarantines it instead of selecting one prompt silently

#### Scenario: Conflicting write has no payload
- **WHEN** both prompt fields are present but empty
- **THEN** validation rejects the write before the empty-record deletion path

#### Scenario: One supported prompt source
- **WHEN** only composer or only staged_prompt is present and otherwise valid
- **THEN** existing draft persistence and local-only recovery remain available

### Requirement: Closed agent drafts release only recoverable runtime state
When the last agent owning a local draft key closes, the runtime SHALL checkpoint pending content and release clean routing/load/cache state without deleting its saved draft. Failed checkpoints SHALL retain the latest content and respect retry backoff.

#### Scenario: Reopen a saved session draft
- **WHEN** a session is closed and later reopened under a new AgentId
- **THEN** its saved local draft is restored instead of being skipped by an obsolete loaded marker

#### Scenario: Checkpoint fails before reopen
- **WHEN** closing cannot save the latest draft and that key is reopened
- **THEN** retained latest content remains recoverable and is not replaced by stale disk content or an empty composer

#### Scenario: Shared draft ownership
- **WHEN** one agent closes while another agent still owns the same key
- **THEN** shared draft runtime state is not retired prematurely

### Requirement: Local draft invalidation retries without restoring stale content
Within a running pager, failed local draft invalidations SHALL retain per-key retry intent with backoff, including after agent closure or session binding. Pending invalidation SHALL prevent stale disk content from being restored. A newly captured valid draft with payload SHALL supersede the pending invalidation for its key.

#### Scenario: Prompt ownership transfers while deletion fails
- **WHEN** an ACP prompt RPC transfers ownership and removal fails for its current or session draft key
- **THEN** retain and retry each failed deletion without treating old content as recoverable or blocking the RPC on successful deletion

#### Scenario: Repeated unsupported input
- **WHEN** capture continues to reject the same current input while deletion is pending
- **THEN** preserve a future retry deadline without per-tick deletion attempts or postponing it on every sync

#### Scenario: New draft supersedes removal
- **WHEN** valid new content is captured for a key with pending invalidation
- **THEN** cancel that key's old deletion intent so its retry cannot remove the new draft

#### Scenario: Binding or reopening with pending deletion
- **WHEN** an invalidated key loses its agent owner or binds from cwd to session before removal succeeds
- **THEN** keep deletion obligations for the original keys and do not restore or migrate their stale content into the new composer

### Requirement: Export path completion bounds directory enumeration
Export path completion SHALL inspect at most1,000 directory iterator results per request, counting hidden entries and read errors toward that limit, and SHALL return at most100 visible suggestions.

#### Scenario: Directory contains mostly hidden entries or errors
- **WHEN** directory iteration yields hidden names or failed entries before visible files
- **THEN** those results consume the enumeration budget and the iterator is not advanced beyond1,000 results

#### Scenario: Normal visible entries
- **WHEN** visible files and directories occur within the budget
- **THEN** existing prefix insertion, directory suffixes and directory-first sorting remain intact

### Requirement: Scroll recorder rejects special file targets
The scroll recorder SHALL accept only regular file targets, checking the opened handle before truncation. Unix FIFO opening SHALL not wait for a reader. Rejected targets SHALL use the existing disabled-recorder failure state.

#### Scenario: FIFO target
- **WHEN** an explicit scroll-log target is a FIFO, with or without a reader
- **THEN** first recording fails without waiting for a FIFO peer and disables recording without writing JSONL to that pipe

#### Scenario: Existing regular target
- **WHEN** the target is a regular file or a symbolic link to one
- **THEN** preserve lazy opening and replacement of previous file contents on first record

### Requirement: Scroll recording bounds each capture by complete lines
Each scroll recorder SHALL accept at most67,108,864 encoded bytes including newline separators. It SHALL check capacity before each line, preserve previously accepted complete lines, flush buffered content when stopping for the limit, and report inactive without automatic rotation.

#### Scenario: Exact limit or next line exceeds capacity
- **WHEN** a line exactly exhausts the allowance or the next complete line would exceed it
- **THEN** recording stops at a complete-line boundary and previously accepted buffered lines are flushed

#### Scenario: First line cannot fit
- **WHEN** the first line exceeds the allowance
- **THEN** disable before opening or truncating the target

#### Scenario: Restart recording
- **WHEN** recording is toggled on again after its budget was exhausted
- **THEN** the newly constructed recorder receives a fresh allowance without removing prior capture files

### Requirement: Input diagnostic dumps describe the input owner
Input diagnostic recording and dumping SHALL describe the agent view that owns the inspected input surface, including child views and dashboard-attached sessions. Diagnostic routing SHALL not change the business action produced by the key event and SHALL preserve printable-character redaction.

#### Scenario: Child composer handles input
- **WHEN** a key is delegated to a child view and a dump is requested from that surface
- **THEN** diagnostic session and textarea metadata correspond to that child rather than the parent composer

#### Scenario: Parent intercepts input
- **WHEN** a parent overlay or close action consumes input before child delegation
- **THEN** its diagnostic observation is not attributed to an unchanged child textarea

#### Scenario: Dashboard attached surface
- **WHEN** an input dump action targets a dashboard-attached session
- **THEN** a nonempty diagnostic snapshot can be produced for that surface instead of silently returning because the top-level view is the dashboard

### Requirement: Pager log dispatch preserves initialized ownership
Pager log forwarding SHALL use its successfully initialized transport and runtime independently of the calling thread. Flush before initialization SHALL preserve buffered entries, and repeated initialization SHALL not create additional periodic flush tasks.

#### Scenario: Plain thread dispatch
- **WHEN** a thread without an entered Tokio runtime emits a flushable log batch after initialization
- **THEN** forwarding uses the initialization runtime instead of discarding the batch because the producer lacks a runtime

#### Scenario: Flush before initialization
- **WHEN** buffered entries exist and a flush is requested before sender initialization
- **THEN** the entries remain available to the eventual initialized forwarder

#### Scenario: Repeated initialization
- **WHEN** initialization is called after a forwarder is already installed
- **THEN** the existing owner is retained without starting another periodic consumer

### Requirement: Pager log flush has a bounded delivery wait
An initialized pager log flush SHALL stop waiting for its current batch after two seconds if ACP acknowledgement has not completed. An acknowledged or failed send SHALL finish without waiting out that deadline. Timeout SHALL not retry the batch or claim cancellation of already enqueued remote processing.

#### Scenario: Peer retains acknowledgement
- **WHEN** the peer holds a log notification without acknowledging it
- **THEN** the current-batch flush releases its local wait after the two-second budget

#### Scenario: Peer acknowledges or disconnects
- **WHEN** the current log batch is acknowledged or its channel fails
- **THEN** flush completes without waiting for the deadline

### Requirement: Recap admission reflects command enqueue
An enabled recap request SHALL report acceptance only after its session command is queued. A closed command channel SHALL produce an ACP error for manual and automatic requests instead of an accepted response.

#### Scenario: Closed session actor channel
- **WHEN** the recap session exists but its command receiver is closed
- **THEN** recap admission returns an ACP error rather than claiming a recap was queued

#### Scenario: Live session actor channel
- **WHEN** the recap command is successfully enqueued
- **THEN** admission returns success and preserves the requested auto flag

### Requirement: Pager consumes recap admission outcomes
Pager SHALL await an asynchronous recap only after a valid accepted extension response. Disabled, rejected or invalid admission responses SHALL clear manual recap progress through the requesting session's failure path. Automatic failures SHALL remain silent and SHALL not clear a manual request's progress.

#### Scenario: Feature disabled after initialization
- **WHEN** shell responds to a manual recap with result.disabled=true
- **THEN** pager clears that session's manual progress rather than waiting for a recap notification

#### Scenario: Valid accepted response
- **WHEN** the extension response confirms ok=true and does not disable recap
- **THEN** manual progress remains until the asynchronous result arrives

#### Scenario: Automatic admission fails
- **WHEN** an automatic recap response is disabled, rejected or invalid
- **THEN** pager does not show a toast or clear existing manual progress

### Requirement: Replayed recaps do not settle current feedback
Pager SHALL restore historical recap blocks without marking the current away period satisfied or clearing current manual recap progress. Live recap notifications SHALL retain their existing feedback behavior.

#### Scenario: Historical recap restored
- **WHEN** an admitted replay contains an automatic or manual recap
- **THEN** its block is restored while current automatic eligibility and manual progress remain unchanged

#### Scenario: Live recap received
- **WHEN** a live recap is displayed
- **THEN** it marks the away period satisfied and clears manual progress only for a manual recap

### Requirement: Automatic recap bookkeeping is session scoped
Within an away period, showing a recap or attempting automatic recap for one session SHALL not suppress another session's eligibility. Polling and focus-return eligibility SHALL use the active root session identity. A new away period SHALL reset all session recap bookkeeping while retaining the existing timing thresholds.

#### Scenario: Background result arrives
- **WHEN** a live recap is displayed for a background session
- **THEN** only that session's shown state is consumed

#### Scenario: Another session is in backoff
- **WHEN** one session has recently attempted automatic recap
- **THEN** the retry delay applies only to that session

#### Scenario: New away period
- **WHEN** focus is lost for a new away period
- **THEN** prior per-session shown and retry state is cleared

### Requirement: Hidden announcement state commits complete snapshots
Hidden announcement state persistence SHALL publish complete canonical snapshots without truncating the current destination during preparation. A failed commit SHALL propagate failure to the pager persistence result and clean up only its own temporary artifacts.

#### Scenario: Snapshot write succeeds
- **WHEN** a hidden-ID snapshot is committed
- **THEN** the destination contains a complete canonical hidden_ids document

#### Scenario: Preparation fails
- **WHEN** writing or preparing the replacement fails before publication
- **THEN** the prior committed destination is preserved and failure is reported

### Requirement: Hidden announcement state has bounded IO admission
Hidden announcement state loading SHALL accept only ordinary files containing at most1,048,576 encoded bytes and SHALL check both metadata and actual bytes read. Rejected input SHALL leave all announcements visible without modifying the source. Persistence SHALL reject snapshots exceeding the same limit before replacing existing state.

#### Scenario: Oversized or special source
- **WHEN** the state source exceeds the byte limit or is not an ordinary file
- **THEN** loading yields no hidden IDs without modifying the source; Unix FIFO admission does not wait for a writer

#### Scenario: Source exceeds its observed size
- **WHEN** actual bytes read exceed the allowance despite an earlier acceptable metadata observation
- **THEN** the bounded reader rejects before parsing more than allowance plus one bytes

#### Scenario: Oversized snapshot write
- **WHEN** an encoded hidden-state snapshot exceeds the allowance
- **THEN** persistence returns an error without replacing the prior committed state

### Requirement: Announcement preference writes follow local change order
A running pager AppView SHALL have at most one announcement preference write in flight. Changes during that write SHALL be coalesced into the latest hidden-ID state, submitted after the earlier write completes. Completion failure SHALL not prevent an already pending newer state from being submitted.

#### Scenario: Hide and show while a write is pending
- **WHEN** hidden IDs change repeatedly before the in-flight write completes
- **THEN** no overlapping write starts and completion submits only the latest state

#### Scenario: Earlier write fails
- **WHEN** an in-flight write fails with newer changes pending
- **THEN** its error remains reported and the newer state is submitted next

#### Scenario: Update prunes hidden IDs
- **WHEN** an announcement update prunes hidden IDs during another write
- **THEN** the pruned state participates in the same serialized scheduling

### Requirement: Debug latest-link updates preserve unowned temporaries
Unix debug latest-link updates SHALL NOT remove an existing temporary entry before symlink creation. A failed creation SHALL preserve that entry and the current latest link. Cleanup after failed publication SHALL only be attempted after this update successfully created the temporary link.

#### Scenario: Temporary path collision
- **WHEN** a regular file or symlink already occupies the latest-link temporary path
- **THEN** updating latest leaves both the pre-existing entry and current latest target unchanged.

#### Scenario: Publication failure
- **WHEN** temporary creation succeeds but renaming over latest fails
- **THEN** the newly created temporary is removed and the blocking destination remains.

### Requirement: Debug routing retains only selected writer guards
For concurrent first writes to a debug routing sink, only the selected writer guard SHALL be parked for process lifetime. Unselected writer guards SHALL be dropped outside the routing mutex so accepted first lines can flush and redundant workers retire. File opening SHALL remain outside that mutex.

#### Scenario: Concurrent first writes
- **WHEN** several threads first write to the same session sink or fallback sink
- **THEN** one guard per sink remains parked after those writes return, and successfully queued lines remain available after flushing.

### Requirement: Debug pruning respects live cooperating writers
Debug file writers SHALL hold shared advisory locks for their file lifetime. Age-based pruning of ordinary log files SHALL acquire a nonblocking exclusive lock and recheck opened-file age before removal; unavailable locks SHALL cause the file to be spared.

#### Scenario: Old idle writer
- **WHEN** an ordinary log is older than retention but a cooperating writer still owns its shared lock
- **THEN** pruning from another process preserves the log path.

#### Scenario: Writer retired
- **WHEN** all writer handles close and the ordinary file is still older than retention
- **THEN** pruning may acquire exclusive ownership and remove it.

### Requirement: Unified log trimming reads a bounded tail
Unified log trimming SHALL read at most MAX_SIZE / 2 bytes (2.5 MiB) from the later of the opened file midpoint and its final MAX_SIZE / 2 bytes. It SHALL retain only bytes after the first newline in that window, preserve the inode, and leave the file unchanged if no newline exists there.

#### Scenario: Oversized log
- **WHEN** a log is much larger than MAX_SIZE and has complete lines in its trailing window
- **THEN** trimming reads no more than MAX_SIZE / 2 bytes and retains recent complete lines within that budget.

#### Scenario: No line boundary
- **WHEN** the admitted trailing window contains no newline
- **THEN** trimming leaves the source unchanged.

### Requirement: Unified writer identity belongs to the opened handle
Unified log writers SHALL capture their initial identity from the opened descriptor rather than a subsequent path lookup. On Unix, maintenance SHALL compare this captured device/inode with the current path identity, allowing recovery when replacement or removal occurs between open and writer construction.

#### Scenario: Replacement during open initialization
- **WHEN** a log path is replaced or removed after its descriptor opens but before writer initialization finishes
- **THEN** the writer tracks the old descriptor identity and the next due maintenance can reopen the live path for visible writes.

### Requirement: Image normalization workers have process-wide admission
The shell normalization blocking adapter SHALL run at most one normalize/transcode closure at a time per process. Its permit SHALL remain owned by the blocking closure until that closure returns or unwinds, even when its async caller is canceled.

#### Scenario: Caller canceled during decode
- **WHEN** a blocking image normalization task continues after its async waiter is canceled and another normalization is requested
- **THEN** the new task waits until the existing blocking task finishes before entering its closure.

### Requirement: Session rename participates in lifecycle serialization
Within one agent, session rename SHALL share the per-session lifecycle gate with load and delete from authoritative lookup through title commit. It SHALL preserve the distinction between resident actor mutation and dormant storage mutation without waiting on a loader queued behind its own guard.

#### Scenario: Load or delete competes with rename
- **WHEN** rename owns the session lifecycle guard
- **THEN** same-agent load/delete cannot cross its lookup and title commit boundary

#### Scenario: Loader announced behind rename
- **WHEN** a load is announced but awaits the lifecycle guard held by rename
- **THEN** rename resolves current residency without awaiting that blocked load and can complete

### Requirement: Session cleanup TTL cannot wrap or overflow
Session cleanup SHALL establish safe current deletion eligibility before removing history.

#### Scenario: Oversized configured retention
- **WHEN** configured TTL cannot be represented safely
- **THEN** cleanup reports the invalid policy and does not delete session history

#### Scenario: Cutoff cannot be represented
- **WHEN** subtracting the selected retention would exceed supported date bounds
- **THEN** cleanup fails without panic or deletion

#### Scenario: Default retention
- **WHEN** no cleanup TTL is configured
- **THEN** the existing 30-day policy applies

### Requirement: Session cleanup rechecks activity under writer ownership
Session cleanup SHALL establish safe current deletion eligibility before removing history.

#### Scenario: Activity refreshed after initial scan
- **WHEN** a candidate looked expired during scanning but its same-entity activity was refreshed before cleanup acquired the writer lease
- **THEN** cleanup preserves it using the fresh activity timestamp

#### Scenario: Fresh eligibility cannot be established
- **WHEN** cleanup cannot read and validate current activity under its lease
- **THEN** it does not delete that candidate based on the earlier snapshot

### Requirement: Automatic session cleanup updates search projection
Successful automatic session cleanup SHALL schedule eviction of deleted session identities from the search projection for the same storage root. Preserved or failed candidates SHALL NOT be reported as successfully deleted. Eviction SHALL use the existing asynchronous missing-summary handling without creating an absent index.

#### Scenario: Expired indexed session is removed
- **WHEN** automatic cleanup successfully removes an expired session
- **THEN** its identity is queued for search update and the missing-summary path removes its index document

#### Scenario: Candidate is retained
- **WHEN** a candidate is fresh, explicitly skipped, writer-owned elsewhere or fails cleanup
- **THEN** cleanup does not report it as successfully deleted for eviction

#### Scenario: No search index exists
- **WHEN** a cleanup deletion is processed without an existing index
- **THEN** eviction does not create an index solely to remove that document

### Requirement: Retry contended session search projections
A live search worker SHALL retain and retry a session projection update or eviction that fails specifically because SQLite remains Busy or Locked after its own wait. Retry SHALL use a nonzero delay and the existing per-root coalescing queue. Non-contention failures SHALL NOT be treated as lock contention.

#### Scenario: Lock clears after failed eviction
- **WHEN** an indexed session has been deleted and its queued eviction encounters SQLite lock contention
- **THEN** the worker retains the key and retries after a delay, allowing eviction after lock release without another notification.

#### Scenario: Projection succeeds
- **WHEN** a pending update or eviction succeeds
- **THEN** that pending work is removed rather than repeated indefinitely.

#### Scenario: Permanent error
- **WHEN** an update fails with invalid session data or another non-contention error
- **THEN** the worker reports the failure without entering the contention retry loop.

### Requirement: Search version reads distinguish absence from failure
Search index initialization SHALL treat an absent schema-version row as uninitialized, but SHALL propagate a failed schema-version query or decoding operation before continuing document-schema initialization or version stamping.

#### Scenario: Version cannot be decoded
- **WHEN** an existing schema-version row cannot be decoded as its required SQL text type
- **THEN** opening the index returns the error without restamping that row as the current version.

#### Scenario: Fresh metadata
- **WHEN** the index has no meta table or no version row
- **THEN** initialization establishes the existing meta table and can initialize the index normally.

#### Scenario: Readable version
- **WHEN** the version row is successfully read
- **THEN** the existing current/older/newer/readable-malformed version policy remains in effect.

### Requirement: Search quarantine failure stops recreation
Search-cache recovery SHALL stop before recreation when a required quarantine rename fails. Missing source files SHALL be distinguished from unsuccessful isolation. Recovery diagnostics SHALL distinguish an isolated cache from a successfully recreated cache.

#### Scenario: Isolation fails
- **WHEN** a main database or sidecar cannot be moved into quarantine
- **THEN** that recovery attempt reports the isolation failure and does not call recreate at the live path.

#### Scenario: Recreation fails after isolation
- **WHEN** quarantine changes the cache namespace but recreation fails
- **THEN** recovery preserves quarantine artifacts, exposes the namespace change to existing invalidation, and does not report successful empty-cache recreation.

#### Scenario: Isolation completes
- **WHEN** required files are successfully isolated or confirmed absent
- **THEN** recovery may attempt recreation through the existing path.

#### Scenario: Caller observes failed recovery
- **WHEN** isolation or recreation fails
- **THEN** the index caller returns its triggering error instead of unconditionally reopening the live path after failed healing.

### Requirement: Search bootstrap bounds live Timeline readers
Search bootstrap SHALL acquire its configured concurrency capacity before opening each Timeline reader. Capacity SHALL remain held until both the admitted indexing task and any detached blocking Timeline read finish. Identity-checked snapshot readers SHALL continue to be used.

#### Scenario: Session count exceeds descriptor budget
- **WHEN** many sessions are bootstrapped with sufficient descriptors for the configured concurrent work but fewer descriptors than sessions
- **THEN** bootstrap processes their ledgers without retaining one open reader per session.

#### Scenario: Blocking read outlives timeout
- **WHEN** an admitted blocking Timeline read continues after its async timeout
- **THEN** its concurrency capacity remains occupied until the blocking read finishes.

#### Scenario: Reader admission fails
- **WHEN** the storage authority rejects opening a reader
- **THEN** bootstrap reports failure without publishing a completed-bootstrap marker.

#### Scenario: Read-only bootstrap visits many session directories
- **WHEN** Timeline readers are created for search projection
- **THEN** their transient directory capabilities are not retained in the adapter writer cache after reader construction.

#### Scenario: Bootstrap enumerates summaries
- **WHEN** bootstrap lists session summaries
- **THEN** enumeration retains summary values rather than a directory capability per result, without populating the writer cache or weakening identity validation.

### Requirement: TTL cleanup bounds candidate handle ownership
TTL cleanup SHALL discover summaries without retaining a directory handle per result and SHALL process candidates with operation-scoped handle and maintenance lease ownership. Discovery SHALL complete before any deletion. A discovery snapshot SHALL NOT authorize deletion without current same-entity eligibility checks.

#### Scenario: History exceeds descriptor capacity
- **WHEN** the number of sessions exceeds the process descriptor limit but sequential cleanup has sufficient capacity
- **THEN** cleanup can process all eligible candidates without retaining one handle per discovered session.

#### Scenario: Candidate is preserved or rejected
- **WHEN** current candidate state is fresh, hidden, writer-owned, invalid or cannot be opened
- **THEN** cleanup preserves it, emits no deletion callback and releases operation-owned resources.

#### Scenario: Discovery rejects duplicate identity
- **WHEN** the complete discovery scan detects duplicate canonical session identities
- **THEN** cleanup fails before deleting any candidate.

### Requirement: Session scans propagate operational failures
Session discovery SHALL distinguish confirmed missing or invalid entries from operational failures. NotFound and InvalidData candidate errors MAY be skipped; other directory-open, summary-read or physical-identity-read errors SHALL fail the scan rather than return a successful partial result.

#### Scenario: Candidate cannot be inspected
- **WHEN** a candidate directory, summary or required CWD marker cannot be read due to permission or another operational failure
- **THEN** discovery returns the error and cleanup does not start deleting from a partial candidate set.

#### Scenario: Invalid or vanished candidate
- **WHEN** a candidate is confirmed missing or invalid
- **THEN** discovery retains the existing skip behavior while processing other valid entries.

#### Scenario: Search discovery fails
- **WHEN** search bootstrap receives a discovery error
- **THEN** that attempt does not proceed to orphan pruning or completed-bootstrap publication using partial results.

### Requirement: Rewind operations reject historical load failure
Full-history rewind queries and mutations SHALL return a failed historical load rather than operate on an incomplete in-memory projection. The source and live points SHALL remain available for retry. Shell rewind and pending rewind recovery SHALL propagate this failure before applying their dependent effects.

#### Scenario: Historical read fails
- **WHEN** the deferred rewind source fails to read
- **THEN** full-history queries and mutations report failure without consuming the source or changing live points.

#### Scenario: Source becomes readable
- **WHEN** the same pinned source is repaired after a failed load
- **THEN** a subsequent load merges its historical points with live points using existing live-point precedence.

#### Scenario: Cancel requests an internal rewind
- **WHEN** cancellation needs to rewind its conversation but full rewind history cannot load
- **THEN** that internal rewind reports failure before committing its conversation rewind or truncating checkpoints; already committed cancellation terminal facts are retained.

### Requirement: Pinned rewind parsing rejects malformed records
Pinned rewind JSONL readers SHALL reject a nonblank record that fails deserialization, returning InvalidData with its one-based physical line number rather than a successful partial record set. Historical load failure SHALL retain the source and live points for retry.

#### Scenario: Invalid record among valid records
- **WHEN** a pinned history contains valid records surrounding an invalid record
- **THEN** the reader fails at that record and no valid prefix is merged into the tracker.

#### Scenario: Retry after repair
- **WHEN** that pinned file is repaired
- **THEN** the next full load succeeds and retains existing live-point precedence.

#### Scenario: Non-record whitespace
- **WHEN** a pinned history is empty or includes blank lines and otherwise valid records
- **THEN** blank lines remain accepted and count toward diagnostic line numbers.

#### Scenario: Metadata parse failure
- **WHEN** the pinned metadata reader encounters a record it cannot deserialize
- **THEN** the existing picker fallback uses in-memory points without consuming the historical source.

### Requirement: Rewind file modes consider the affected checkpoint range
Rewind mode selection SHALL consider file changes at the target prompt and all later checkpoints when determining file-mode availability. It SHALL recompute the same range when returning from preview. Inline edit SHALL retain its existing FilesOnly exclusion.

#### Scenario: Later checkpoint has changes
- **WHEN** the selected checkpoint has no own file changes but a later checkpoint does
- **THEN** ordinary rewind offers FilesOnly for the selected target, including after back navigation.

#### Scenario: Changes precede the target
- **WHEN** file changes exist only before the target
- **THEN** those earlier changes do not enable FilesOnly for that target.

#### Scenario: Inline resubmission
- **WHEN** inline edit selects a target with later file changes
- **THEN** file changes are recognized while FilesOnly stays excluded.

#### Scenario: Preselected target bypasses the picker
- **WHEN** points finish loading for a preselected rewind target, including an inline edit target
- **THEN** file-mode eligibility includes all checkpoints at or after the resolved target and retains the inline FilesOnly exclusion.

### Requirement: Rewind back navigation preserves the active target
Returning to rewind mode selection SHALL preserve the current phase's explicit target, including when the cached checkpoint list is missing. Fallback selection SHALL be used only when the current phase does not carry a target.

#### Scenario: Older target preview
- **WHEN** an older checkpoint is selected and its preview confirmation returns to mode selection
- **THEN** the original target remains selected rather than changing to the newest checkpoint.

#### Scenario: Cached list unavailable
- **WHEN** confirmation retains a target but cached checkpoint metadata is unavailable on return
- **THEN** mode selection retains that target.

#### Scenario: Conversation-only confirmation
- **WHEN** the target-zero conversation-only confirmation returns to mode selection
- **THEN** the target remains zero even if newer checkpoints exist.

### Requirement: Rewind reads belong to the requesting interaction
The client SHALL apply points and preview results only to their current requesting interaction, session and phase. Dismissal SHALL invalidate the pending read. Starting a new read SHALL supersede the previous read. Execution results SHALL retain their existing reconciliation behavior.

#### Scenario: Dismissed read completes
- **WHEN** points or preview completes or fails after its interaction was dismissed
- **THEN** it does not reopen an overlay, alter the draft or show a failure toast.

#### Scenario: New interaction supersedes a read
- **WHEN** a new read starts before a previous result arrives
- **THEN** the previous success or failure does not alter the new interaction.

#### Scenario: Session ownership
- **WHEN** a result belongs to a session no longer attached to that agent
- **THEN** the result is ignored.

#### Scenario: Another view is active
- **WHEN** the owning agent and session still await the read while another view is active
- **THEN** the result updates its owning agent normally.

#### Scenario: Current points read fails
- **WHEN** the current points request fails
- **THEN** the interaction closes, restores its stashed draft and reports the failure.

#### Scenario: Session ID is reused after unbinding
- **WHEN** a read was issued before unbinding and the same session ID is subsequently rebound
- **THEN** its result is ignored because it belongs to an earlier binding lifetime.

#### Scenario: Same binding is reiterated
- **WHEN** the current session ID is bound again without an intervening unbind or identity change
- **THEN** a pending matching read remains valid.

### Requirement: Trace export does not require valid model configuration
CLI trace dispatch SHALL proceed to session trace validation without requiring successful model configuration loading or AgentConfig construction. Existing session and output errors SHALL remain observable.

#### Scenario: Invalid model config and missing trace target
- **WHEN** model configuration TOML is malformed and trace requests a missing session
- **THEN** the command reports the missing session rather than aborting on model configuration parsing.

### Requirement: Permission persistence uses the shared setting path
Pager SHALL retain PersistSetting for default permission preferences and NotifySessionPermissionMode for current-session changes, without the retired PersistPermissionMode policy and best-effort result variants.

#### Scenario: Default preference write fails
- **WHEN** default permission persistence fails
- **THEN** the shared setting coordinator applies its existing failure/rollback behavior.
