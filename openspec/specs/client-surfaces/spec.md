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
macOS 剪贴板图片 SHALL 经由 `osascript` 子进程和私有临时文件传输。Grow 对编码数据的实际读取 SHALL 限制为最多 50,000,001 字节，并拒绝超过 50,000,000 字节的结果；空结果保持无图片语义。该边界隔离 Grow 进程免受 AppKit `dataForType:` 图片物化分配影响，但不限制 `osascript`/AppKit 子进程内存、图片写入临时文件前的磁盘用量或图片解码内存。元数据探测仍可通过原生 AppKit 读取 `changeCount` 与 `types`，不得读取图片数据。

#### Scenario: Native image exceeds the budget
- **WHEN** the pasteboard advertises an image whose encoded data exceeds 50,000,000 bytes
- **THEN** Grow SHALL NOT request image data through `dataForType:` and SHALL route the explicit image read through `osascript`; the helper-process allocation is outside the Grow-process memory boundary.

#### Scenario: Fallback image file exceeds the budget
- **WHEN** AppleScript writes an encoded image larger than 50,000,000 bytes to its transfer file
- **THEN** Grow reads at most 50,000,001 bytes, returns an error, and cleans up the file through the owned private temporary directory; the oversized result is not treated as an empty image.

#### Scenario: Image fits the budget
- **WHEN** encoded image data is non-empty and no larger than 50,000,000 bytes
- **THEN** Grow returns the full encoded data with the existing MIME/type priority through the AppleScript transfer path.

#### Scenario: Image data is acquired
- **WHEN** `get_image` or `get_attachments` reads an explicit image paste
- **THEN** image bytes are acquired by the `osascript` subprocess and Grow does not call `NSPasteboard dataForType:`; native metadata probes remain data-free.

### Requirement: Clipboard RGBA encoding validates input shape
剪贴板RGBA编码 SHALL 在编码和输出分配前验证非零宽高、u32尺寸可表示性、宽高乘4可表示性及精确字节长度；无效输入返回错误，不因尺寸截断、整数溢出或编码器长度断言panic。

#### Scenario: Invalid dimensions or byte count
- **WHEN** 宽高为零、尺寸或长度计算不可表示，或buffer过短/过长
- **THEN** 返回明确错误，不调用PNG编码器。

#### Scenario: Valid RGBA input
- **WHEN** 尺寸有效且buffer字节数恰好为宽乘高乘4
- **THEN** 保持PNG编码并保留像素内容。

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
History search SHALL retain a coalesced latest pending request and submit updates without waiting for worker queue capacity. Closing its daemon SHALL not wait for notification capacity or ongoing matching. Drop SHALL signal cooperative cancellation of the active request, and the worker SHALL observe that signal between per-item history conversion, scoring, and highlight-index operations, abandoning the request without publishing partial results. A query request SHALL retain at most 100 scored candidates while scanning the corpus, then order those candidates by descending score; the best match SHALL remain last in the rendered result list. A single conversion, scoring, or index operation is indivisible and may finish after Drop returns.

#### Scenario: Worker is busy while inputs accumulate
- **WHEN** item refreshes and queries arrive before the worker can process pending work
- **THEN** submissions return without waiting for channel capacity and the next request retains the latest item refresh with its latest following query

#### Scenario: Closing with pending work
- **WHEN** the history search daemon is dropped while work is pending
- **THEN** stop takes precedence over pending work without blocking the UI on channel capacity

#### Scenario: Closing during active matching
- **WHEN** the history search daemon is dropped while a score, highlight-index operation, or result sort is in progress
- **THEN** Drop returns without waiting for that operation, and the worker abandons the request after the current operation completes without publishing partial results

#### Scenario: Closing during item preprocessing
- **WHEN** the history search daemon is dropped while history items are being converted for matching
- **THEN** Drop returns without waiting for the current conversion, the worker abandons preprocessing after that operation, and no partial item set is published

#### Scenario: Query matches exceed the visible result limit
- **WHEN** more than 100 history items match a query
- **THEN** matching retains only the 100 highest-scoring candidates, orders them from lower to higher score for rendering, and keeps the best match last

### Requirement: History search accepts only current request results
History search SHALL identify results by the originating request and expose only results matching the latest submitted request. Submitting a query or item refresh SHALL invalidate previously selectable results immediately.

#### Scenario: Reopen or change query before completion
- **WHEN** an older request completes after reopening or submitting a newer query
- **THEN** its results cannot be displayed or selected, while the current request result can be applied once

#### Scenario: Item refresh retains navigation intent
- **WHEN** items refresh and the same query is resubmitted after manual navigation
- **THEN** stale results are hidden while waiting and the retained selection index is clamped to current results without resetting the existing navigation intent

### Requirement: Local draft recovery bounds source reads
Local draft recovery SHALL accept only regular file sources, read at most 262,145 bytes, and reject actual content above 262,144 bytes before JSON parsing. Oversized regular files SHALL follow the existing quarantine policy. Immediately before quarantine rename, recovery SHALL verify that the active path still resolves to the source entity opened for validation. If that check observes a replacement, recovery SHALL leave the replacement at the active path and SHALL NOT move or remove it as quarantine for the opened source.
Quarantine publication SHALL fail without replacing an existing quarantine entry.

#### Scenario: File grows after metadata observation
- **WHEN** a draft grows beyond the byte allowance after opening and metadata inspection
- **THEN** bounded reading detects the extra byte and quarantines the draft without restoring its content

#### Scenario: Special file source
- **WHEN** the draft path opens a non-regular file, including a Unix FIFO without a writer
- **THEN** loading returns an error without waiting for a FIFO writer or moving the special file into quarantine

#### Scenario: Regular file at allowance
- **WHEN** valid draft JSON including trailing whitespace exactly fits the allowance, including through a regular-file symlink
- **THEN** recovery preserves the existing record validation and restoration behavior

#### Scenario: Draft path is replaced before quarantine identity check
- **WHEN** recovery reads an invalid or oversized draft from an opened file and the active path is replaced with a different valid draft before the final quarantine identity check
- **THEN** the replacement remains at the active path, is not moved into quarantine, and is not removed

#### Scenario: Quarantine name collides
- **WHEN** a generated quarantine name already belongs to an existing entry
- **THEN** recovery preserves that entry and retries with another name

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
Pager log forwarding SHALL use its successfully initialized transport and runtime independently of the calling thread. It SHALL retain no more than 256 pending entries or 1 MiB of their encoded content, reject an entry whose encoded content exceeds 64 KiB before materializing a full encoding, and forward accepted entries in FIFO batches no larger than 16 entries or 256 KiB through one sender. Flush before initialization SHALL preserve accepted buffered entries within these limits. Repeated initialization SHALL not create another sender. New over-budget entries SHALL be dropped without displacing accepted entries or affecting session state.

#### Scenario: Plain thread dispatch
- **WHEN** a thread without an entered Tokio runtime emits a flushable log batch after initialization
- **THEN** forwarding uses the initialization runtime instead of discarding the batch because the producer lacks a runtime.

#### Scenario: Flush before initialization
- **WHEN** buffered entries exist and a flush is requested before sender initialization
- **THEN** accepted entries remain available to the eventual initialized forwarder within the pending budget.

#### Scenario: Repeated initialization
- **WHEN** initialization is called after a forwarder is already installed
- **THEN** the existing owner is retained without starting another sender.

#### Scenario: Slow peer or oversized entry
- **WHEN** producers exceed a pending count/byte budget before initialization or while the peer holds an acknowledgement, or submit one oversized encoded entry
- **THEN** new over-budget entries are dropped while accepted entries retain their order; pending plus one in-flight batch remains bounded.

### Requirement: Pager log flush has a bounded delivery wait
An initialized Pager `flush_blocking` SHALL wait for all entries accepted before the call to settle through its one sender, including earlier batches already removed from the pending queue. Its total local wait SHALL stop after two seconds if ACP acknowledgement has not completed. Acknowledged, failed or timed-out sends SHALL not be retried by this forwarder; settlement SHALL NOT claim remote persistence or cancellation of already enqueued remote processing.

#### Scenario: Peer retains acknowledgement
- **WHEN** the peer holds a log notification without acknowledging it, including one dispatched before `flush_blocking`
- **THEN** the flush releases its local wait after the two-second budget without treating the earlier batch as delivered.

#### Scenario: Peer acknowledges or disconnects
- **WHEN** all prior accepted log batches are acknowledged or their channel fails
- **THEN** flush completes without waiting for the deadline.

#### Scenario: Earlier batch remains in flight
- **WHEN** a count-triggered or periodic batch is in flight and another entry is accepted before shutdown flush
- **THEN** flush does not complete until the earlier batch and the later accepted entry both settle, unless its two-second wait expires.

#### Scenario: Concurrent flushes
- **WHEN** two callers flush the same accepted frontier while its batch awaits acknowledgement
- **THEN** both wait for the same settlement without duplicating the batch.

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
Within an away period, showing a recap or attempting automatic recap for one session SHALL not suppress another session's eligibility. Polling and focus-return eligibility SHALL use the active root session identity. A new away period SHALL reset all session recap bookkeeping while retaining the existing timing thresholds. Each automatic request SHALL carry the owning away-period ID, and its successful live notification SHALL echo that ID. Pager SHALL display and count a live automatic recap only when the ID matches its current away period. A replayed recap SHALL restore display without changing current eligibility; manual recap behavior SHALL remain independent of this automatic request identity.

#### Scenario: Background result arrives
- **WHEN** a live recap is displayed for a background session in the current away period
- **THEN** only that session's shown state is consumed

#### Scenario: Another session is in backoff
- **WHEN** one session has recently attempted automatic recap
- **THEN** the retry delay applies only to that session

#### Scenario: New away period
- **WHEN** focus is lost for a new away period
- **THEN** prior per-session shown and retry state is cleared, and a new ID is minted

#### Scenario: Old automatic result arrives in a new away period
- **WHEN** an automatic recap for away period A arrives live after period B has begun
- **THEN** Pager does not display the A result or mark B shown, and B remains eligible under its own backoff

#### Scenario: Current automatic result arrives after focus return
- **WHEN** a recap for the current away period arrives after focus returns but before another focus loss
- **THEN** Pager may display it if the session remains eligible and counts it against that period

#### Scenario: Historical or manual recap
- **WHEN** a historical recap is replayed or a manual recap is produced
- **THEN** replay affects only display, and manual feedback follows the manual path without requiring an away-period ID

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
The shell image normalization pipeline for each `normalize_one` call SHALL run in one process-wide single-worker blocking closure. That closure SHALL include input base64 decoding, optional endpoint transcoding and its PNG base64 encoding, normalization validation/decoding/re-encoding, and output base64 encoding. Its permit SHALL remain owned by the closure until every stage returns or unwinds, even when its async caller is canceled.

#### Scenario: Normalization stages share admission
- **WHEN** an image requires optional endpoint transcoding and/or compression
- **THEN** input decode, conversion, compute, and output encoding all execute while the same worker permit is held

#### Scenario: Caller canceled during decode
- **WHEN** a normalization closure continues after its async waiter is canceled and another image normalization is requested
- **THEN** the new image waits until every stage in the existing closure finishes before entering its own normalization pipeline.

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
Pinned rewind JSONL readers SHALL reject a nonblank record that fails deserialization, returning InvalidData with its one-based physical line number rather than a successful partial record set. Historical load failure SHALL retain the source and live points for retry. Metadata scans SHALL validate nested snapshot types without retaining file content and SHALL report a parse failure to the picker.

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
- **WHEN** the pinned metadata reader encounters a record it cannot deserialize, including a malformed nested snapshot
- **THEN** the picker request reports failure without consuming the historical source or presenting in-memory points as a complete checkpoint list.

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
The client SHALL apply points and preview results only to their current requesting interaction, session and phase. Dismissal SHALL invalidate the pending read. Starting a new read SHALL supersede the previous read. Execution results SHALL follow the separate session-binding reconciliation requirement.

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

### Requirement: Restore feedback does not require an unused degree cache
Pager SHALL present code-restoration summaries and failures without storing restore_degree in AgentSession or forwarding it through internal completion actions. Shared RestoreDegree wire types, parsing and validation SHALL remain intact.

#### Scenario: Loaded or forked restoration result
- **WHEN** session loading or worktree forking completes with a code-restoration summary
- **THEN** Pager displays the existing success or failure feedback and performs the existing follow-up actions without caching restore degree.

### Requirement: Terminal client excludes the entertainment game
Pager SHALL NOT register or run the GBOOM game, its renderer, game simulation clock or exclusive keyboard enhancement layer. Shared image viewing, terminal keyboard restoration, animation clocks, focus handling and debug diagnostics SHALL remain available.

#### Scenario: Command registry without game
- **WHEN** Pager builds its built-in command registry
- **THEN** no gboom command is registered and supported debug commands remain registered.

#### Scenario: Input and terminal operation after game removal
- **WHEN** a user views an image, changes focus or closes the terminal UI
- **THEN** existing image controls and common terminal/input lifecycle behavior remain intact without game-owned state or clocks.

### Requirement: Image numbering does not emit unused dedicated metadata
ACP image construction SHALL omit the unused grow.dev/imageDisplayNumber metadata key. Visible image numbers and textual image anchors, image bytes, URIs, durable identities and generic ACP metadata preservation SHALL remain unchanged.

#### Scenario: Recovered image retains its visible identity
- **WHEN** a numbered image anchor accompanies an explicit attachment submitted or restored by the client
- **THEN** the textual anchor preserves its number and the attachment preserves its content and URI without emitting dedicated display-number metadata.

#### Scenario: Other metadata survives normalization
- **WHEN** an ACP image with unrelated metadata is normalized or admitted
- **THEN** generic metadata remains preserved by the existing normalization and admission paths.

### Requirement: Scroll diagnostics use the debug command entry
Pager SHALL expose the existing scroll HUD toggle through /debug scroll and SHALL NOT register the redundant /scroll-debug alias. The HUD, environment enablement, FPS and scroll logging SHALL remain available.

#### Scenario: Toggle scroll diagnostics
- **WHEN** the user executes /debug scroll
- **THEN** Pager dispatches the existing ToggleScrollDebugHud action and HUD hints reference /debug scroll.

### Requirement: Announcement metadata omits inactive persistence flag
Announcements SHALL NOT expose the unused persistent field. Dismissibility, expiry and hidden-announcement persistence SHALL retain their existing behavior.

#### Scenario: Default announcement payload
- **WHEN** default announcements are serialized for clients
- **THEN** no persistent field is emitted and supported announcement display controls remain present.

### Requirement: Discoverable historical conversation search
Pager SHALL expose `/history [query]` through the existing session picker and session content index, including assistant results and content snippets. `/history --prompts` SHALL retain prompt recall and composer insertion. No separate conversation index SHALL be introduced.

#### Scenario: Search historical answer
- **WHEN** the user runs `/history deployment error`
- **THEN** the session picker opens with that query and requests content results immediately, showing matching snippets through the existing picker and allowing the selected session to be resumed.

#### Scenario: Browse before searching
- **WHEN** the user runs `/history` without a query
- **THEN** the picker opens with recent sessions and focused search input; editing the query uses the existing debounced content search.

#### Scenario: Recall a prompt
- **WHEN** the user runs `/history --prompts`
- **THEN** the existing prompt-history overlay opens and accepting a match inserts its text into the composer.

### Requirement: Conversation search failures and stale completions
Pager SHALL show a diagnostic for the current content search when the index is disabled, the request fails or times out. A failed search SHALL NOT be represented solely as an empty successful result. Completions from a closed picker or superseded query SHALL NOT mutate results or show diagnostics in the current picker.

#### Scenario: Index disabled
- **WHEN** the current content search returns the index-disabled diagnostic
- **THEN** loading ends and the user sees the diagnostic while the picker remains usable.

#### Scenario: Old search completes after reopen
- **WHEN** a picker is closed and reopened before an older request finishes
- **THEN** the old results and errors are ignored even if the reopened picker has the same query.

### Requirement: Interjection display preserves user text across resume
Mid-turn user input SHALL persist its user-facing text separately from model-only interjection instructions and skill expansion. Resume SHALL display that persisted user text without exposing the runtime envelope. Literal markup authored by the user SHALL remain intact; display SHALL NOT be recovered by parsing model prompt tags.

#### Scenario: Resume after accepted steering
- **WHEN** accepted mid-turn user input is consumed and its display update is replayed
- **THEN** the display text is the sanitized original input while model context retains its interjection envelope.

#### Scenario: Direct interjection injection
- **WHEN** an interjection without queued input identities is injected
- **THEN** the same display/model separation applies and image blocks remain available.

#### Scenario: User authors prompt-like markup
- **WHEN** the user input includes literal user_query tags
- **THEN** those tags remain part of the user text and are not parsed or removed as runtime framing.

### Requirement: Paste collection preserves input-batch boundaries
A detected unbracketed multiline paste SHALL remain one pending insertion across bounded input collection passes. Reaching an event budget SHALL yield without committing the paste prefix. The existing idle boundary SHALL flush the complete insertion even without a subsequent input event. A completed bracketed `Event::Paste` SHALL retain its own boundary and SHALL NOT absorb or discard later events merely because they were collected in the same input batch.

#### Scenario: Large paste exceeds collection budget
- **WHEN** one continuous multiline paste exceeds the input extension event budget and ends with an unterminated line
- **THEN** its complete text is inserted once as one paste, without separately routed tail keystrokes or multiple chips.

#### Scenario: Exactly exhausted batch becomes idle
- **WHEN** collection reaches its event budget and no further input arrives
- **THEN** the idle deadline flushes the pending paste without requiring another key.

#### Scenario: Event-loop fairness
- **WHEN** a large paste is still arriving
- **THEN** each collection pass retains its bounded event budget and returns to the main loop.

#### Scenario: Ordinary input remains ordinary
- **WHEN** input is a normal key, a completed bracketed paste, or a non-paste event storm
- **THEN** existing key routing and bounded handling remain unchanged.

#### Scenario: Control key interrupts pending collection
- **WHEN** a control key arrives during detection or at the next pending-paste batch
- **THEN** collection returns without waiting for the remaining paste tail and retains the control key for normal routing.

#### Scenario: Bracketed paste followed by ordinary input
- **WHEN** one input batch contains a completed bracketed paste followed by Enter, text, navigation or a control key
- **THEN** Pager routes the paste and each subsequent key separately in arrival order without adding the keys to paste text or dropping them.

#### Scenario: Two completed bracketed pastes
- **WHEN** one input batch contains two completed `Event::Paste` values
- **THEN** Pager preserves two paste events in their original order rather than combining their content.

### Requirement: Usage exposes provider-model totals and cache-hit rates
`/usage` SHALL display total token consumption and provider/model breakdowns using full input plus output including cache-hit input. It SHALL show cached input and cache-hit percentages overall and per provider/model, within the labeled ledger reporting window. The identity SHALL be captured from the selected catalog route before sampling, not from provider-returned model aliases.

#### Scenario: Switch provider or model
- **WHEN** calls use different provider/model identities, including providers serving the same wire model name
- **THEN** prior charges remain in their original groups and overall usage is their cumulative sum.

#### Scenario: Weighted total rate
- **WHEN** models have different input volumes and cache hits
- **THEN** overall rate is total cache-hit input divided by total input, rather than the arithmetic mean of model percentages; each model shows its own total, input, output, cached input and rate.

#### Scenario: Single model and empty input
- **WHEN** only one model has usage or an entry has zero input
- **THEN** the model identity is still shown and a zero denominator displays N/A rather than a fabricated percentage.

#### Scenario: Incomplete or invalid usage
- **WHEN** the ledger is incomplete or cached input exceeds reported total input
- **THEN** incomplete rates are identified as based on recorded usage and invalid ratios display N/A; no misleading exact overall percentage is asserted.

#### Scenario: Existing display paths
- **WHEN** usage is opened in fullscreen, inline or minimal mode
- **THEN** the same statistics projection is shown, and `/session-info` remains unchanged.

### Requirement: Goal tool success closes its running UI row
CreateGoal、GetGoal 与 UpdateGoal 成功结果 SHALL 投影为带原工具调用 id 和结构化输出的 ACP Completed 更新，使客户端在当前 turn 内停止对应工具行的运行状态与计时。

#### Scenario: Read or create completes while the turn continues
- **WHEN** Goal 查询或创建成功，模型随后继续推理或执行其他工具
- **THEN** 对应 Goal 工具行立即完成，不把后续工作耗时记为该工具的读取或创建耗时。

#### Scenario: Goal update or failure
- **WHEN** Goal 状态更新成功或工具返回错误
- **THEN** 既有成功 Completed 和错误 Failed 语义保持不变，不把错误伪装成成功。

证据入口：`crates/codegen/shell/src/session/acp_conversion.rs::acp_tool_update`、`crates/codegen/pager/src/acp/tracker.rs`。

### Requirement: Detailed token counts use comma grouping
Usage 与 Goal 详情中的完整 token 数字 SHALL 使用每三位逗号分隔，并保留不完整账本的 ≥ 标记；预算及未分类历史 SHALL 使用相同格式。紧凑状态栏可以保留 k/M 单位。

#### Scenario: Large incomplete Goal usage
- **WHEN** Goal 详情包含累计、缓存命中、缓存未命中、输出、预算或未分类历史的大数
- **THEN** 例如 100000000 显示为 100,000,000，≥ 和分类含义不变。

### Requirement: Ordinary agent status shows session usage
没有 Goal 的普通主会话 Agent 视图 SHALL 在原 Goal 插槽显示本 session 跨 resume 的 lifetime 累计 token 与输入缓存命中率，并在点击时打开 Usage 页。数据 SHALL 来自可恢复 session ledger 的变化投影，不使用定时轮询、context 窗口压力或 prompt 总额累加替代账本。子 Agent 内嵌视图不增加一个指向父会话用量的入口。

#### Scenario: Calls and late child settlement
- **WHEN** 普通会话产生主调用、已归属的子任务消费或不完整标记
- **THEN** 状态栏按账本的 lifetime 累计 input + output（含 cache hit）更新，重复累计快照不会重复加账，缓存率按总 cached input / 总 input 计算。

#### Scenario: Empty or incomplete usage
- **WHEN** 没有输入、缓存数超过输入或账本不完整
- **THEN** 无效比例显示 N/A；不完整累计显示 ≥，缓存率仅表示 recorded usage，不伪装精确完整用量。

#### Scenario: Open usage or Goal details
- **WHEN** 用户点击普通会话用量或已有 Goal 的状态区域
- **THEN** 普通用量打开既有 Usage 面板的 Usage 页，Goal 继续打开 Goal 详情；窄屏下点击区域不能超出可见区域。

#### Scenario: Resume or reconnect
- **WHEN** 客户端重新连接存活进程或在新 actor incarnation 从持久 Timeline 恢复会话
- **THEN** normal 状态栏显示此前与当前 incarnation 的 lifetime 总计，不重置为零或只显示 resume 后新增用量；resident reconnect 不重复累计。

#### Scenario: Usage details across resumes
- **WHEN** session 至少经历一次有新增模型消费的冷 resume
- **THEN** Usage 页顶部显示 lifetime 总计，并按 Initial run、Resume #1… 展示各 incarnation 的新增消费；各段之和与 lifetime 已知总量一致。

证据入口：`crates/codegen/chat-state/src/actor/state.rs`、`shell/src/extensions/usage.rs`、`pager/src/views/usage_modal.rs`、`pager/src/app/status_blocks.rs`。

### Requirement: Sampling previews are isolated by attempt and delivery capability

采样输出 SHALL 显式声明最终结果交付、可废弃 attempt 预览或不可撤销流的交付能力。可废弃预览 SHALL 在 text、reasoning、工具参数、signature、合并缓冲及终态上保留请求与 attempt 归属，废弃旧 attempt 后才能显示下一 attempt 的内容。不可撤销的响应帧一经外发 SHALL 阻止透明重新采样。多个消费者 SHALL 按实际交付边界采用最严格限制；对可废弃候选，不支持撤回的附加观察者 SHALL 先缓冲到接纳后交付，从而不暴露被废弃的预览。发起不可撤销标准流的客户端 SHALL 保持实时输出及输出后停止恢复的约束。

#### Scenario: Retry after preview output
- **WHEN** 支持 attempt 废弃的 Pager 已展示部分文本和工具参数，随后允许恢复
- **THEN** 旧 attempt 的预览被标为废弃或移除，合并缓冲先处理废弃屏障，新 attempt 独立累积且旧迟到 delta 不污染它。

#### Scenario: Irreversible headless output
- **WHEN** Minimal 的原生终端滚动区、标准 headless 或外部客户端已经收到无法撤销的响应帧
- **THEN** 后续失败终止当前输出，不把新生成内容拼成同一成功响应；只收最终结果模式可依其能力恢复。

#### Scenario: Consumer capability becomes stricter
- **WHEN** 同一请求存在多个消费者或中途接入不支持撤销的消费者
- **THEN** 不假定所有已发布输出可撤销；可废弃候选的未知观察者只在 Accepted 后收到缓冲内容，Discarded 后零候选内容外发，已接纳历史的 load 不重复补发该候选。

#### Scenario: Durable admission is not acknowledged
- **WHEN** 候选流完成但会话接纳尚未确认
- **THEN** 客户端仍将其视为未接纳候选，不发布 Accepted 或可执行工具结果。

#### Scenario: Recovery is denied
- **WHEN** 请求因输出不可撤销、用量不完整、额度耗尽、owner 失效或协议冲突而停止
- **THEN** 终止诊断在既有 attempt evidence 中保留对应停止条件及累计 attempt 数，常规自动恢复保持正常运行活动而不额外弹出警告。

#### Scenario: Reconnect misses the candidate terminal boundary
- **WHEN** Pager 断线期间错过候选的 Accepted 或 Discarded，主会话或复用的子任务视图仍持有未确认预览
- **THEN** 重连丢弃未重新确认的旧预览，只以已接纳内容展示成功历史，不因较新的独立事件 cursor 保留废弃预览；加载失败保留候选归属，再次重连也不能把未确认候选变成已接纳历史。

#### Scenario: Interaction or replay overlaps a sampling candidate
- **WHEN** 候选仍待接纳时到达文件/终端等驱动客户端请求、权限请求或定向历史回放
- **THEN** 原交互与定向路由继续生效，不把请求缓冲到接纳之后或广播给附加观察者；普通交织通知与候选在最终交付时保持原 eventId 顺序。

### Requirement: Generic tool failures render their terminal content once

Pager SHALL assign the terminal content of a generic successful tool call to its output presentation and the terminal content of a generic failed tool call to its error presentation. It SHALL NOT place the same failed content in both fields or render it twice.

#### Scenario: Failed generic tool has diagnostic content
- **WHEN** a generic tool call completes as Failed with nonempty text content
- **THEN** the expanded row shows that text once as the error and has no duplicate output copy.

#### Scenario: Successful generic tool has output
- **WHEN** a generic tool call completes successfully with nonempty text content
- **THEN** the expanded row retains the text as output without manufacturing an error.

### Requirement: Session usage retains agent attribution behind aggregate surfaces

会话用量账本 SHALL 将本会话主 Agent 与每个已结算子 Agent 的已知 token 消费分别归属，同时 SHALL 使总体等于这些消费的累计结果。当前 `/usage`、headless usage 与没有 Goal 的 normal 状态栏 SHALL 只展示既有总体及 provider/model 投影，不公开 Agent 分项。

#### Scenario: Main and child agents consume tokens
- **WHEN** 主 Agent 产生模型消费，两个具有不同 `subagent_id` 的子 Agent 完成并回传各自累计消费
- **THEN** 会话总体包含三者消费各一次，Agent 分项能分别识别主 Agent 与两个子 Agent，并保留每项已知 token 数。

#### Scenario: One child uses multiple models
- **WHEN** 同一子 Agent 的终态累计快照包含多个 provider/model 分项
- **THEN** provider/model 分项继续按模型累计，该子 Agent 分项累计这些模型消费，且会话总体不遗漏或重复任何一项。

#### Scenario: Existing aggregate displays
- **WHEN** 包含子 Agent 消费的会话账本投影到 `/usage`、headless usage 或 normal 状态栏
- **THEN** 当前界面和公共 usage 形状显示包含子 Agent 的总体，但不增加 Agent 分项字段或逐 Agent UI。

#### Scenario: Incomplete child settlement
- **WHEN** 子 Agent 回传已知下界并标记用量不完整，或其用量无法可靠应用
- **THEN** 已知消费仍按原 `subagent_id` 归属，既有 incomplete 规则继续阻止总体被表示成精确完整值。

证据入口：`crates/codegen/chat-state/src/usage.rs`、`crates/codegen/shell/src/session/actor/updates.rs`、`crates/codegen/shell/src/extensions/notification.rs`、`crates/codegen/pager/src/app/status_blocks.rs`。

### Requirement: Cancelling turns reconcile against authoritative prompt state

Pager SHALL 在发送取消后用短时、单请求在途的 prompt-status 对账窗口覆盖 `TurnCancelling`。只有 shell 返回 terminal、unknown 或查询错误时才收敛本地状态；时间本身 SHALL NOT 伪造成功终态。

#### Scenario: Cancel targets a vanished session
- **WHEN** Pager 已进入 `TurnCancelling`，但 shell 不再拥有该 session，因而不会发送取消终态
- **THEN** Pager 在短窗口后查询该 prompt，依据 unknown/error 退出 cancelling、恢复输入，并提示 session/prompt 已不可用。

#### Scenario: Cancel is still being processed
- **WHEN** 权威查询仍返回 running
- **THEN** Pager 保持 `TurnCancelling`，刷新下一次对账窗口，且同一 prompt 同时最多一个查询在途。

#### Scenario: Cancel terminal notification was missed
- **WHEN** 权威查询返回该 prompt 的 terminal 状态
- **THEN** Pager 通过既有 single-finalizer 路径结束原 turn，不重复添加终态或重复推进队列。

证据入口：`crates/codegen/pager/src/app/root/dispatch/turn.rs`、`task_result.rs` 及其 tests。

### Requirement: Failed initial session load returns to its origin

Pager SHALL 把首次 resume 创建的 Agent 视为加载期临时实体。加载失败时 SHALL 删除该临时实体，恢复发起前的 Welcome、Agent 或 Dashboard，并显示可操作的失败信息；不得留下没有 session identity 的可见输入页。

#### Scenario: Resume from welcome fails
- **WHEN** Welcome 中选择的 session 加载失败
- **THEN** 临时 Agent 被删除，界面回到 Welcome，用户可以再次选择并重试。

#### Scenario: Resume from an existing agent or dashboard fails
- **WHEN** 用户从既有 Agent 或 Dashboard 发起 resume 且加载失败
- **THEN** 界面恢复到该发起 surface，既有 Agent 状态不被临时失败页替换。

#### Scenario: In-place reload fails
- **WHEN** 失败结果属于已有 Agent 的原地 reconnect/reload window，而不是首次加载临时 Agent
- **THEN** 继续由既有 reload-window 回滚语义处理，不删除真实 Agent。

证据入口：`crates/codegen/pager/src/app/root/dispatch/session/load.rs` 与 `tests/session/load.rs`。

### Requirement: Context info results are bound to their requesting session view
The client SHALL apply an asynchronous context-info result only when its
session identity and session-binding epoch still match the requesting
AgentView; the check SHALL happen before updating live context state or any
scrollback/modal projection. A zero nonce SHALL retain scrollback intent, and
a nonzero nonce SHALL additionally match the open usage modal epoch as part of
that same pre-mutation validity check.

#### Scenario: Late result after rebinding is ignored
- **WHEN** a context-info request completes after the AgentView is rebound or unbound and rebound
- **THEN** neither live context state nor scrollback or modal state is changed

#### Scenario: Current success and failure retain their surface routing
- **WHEN** a result matches the session identity and binding epoch, and any nonzero nonce matches the open usage modal
- **THEN** success updates live context before routing to zero-nonce scrollback or matching modal, while failure routes only to that same surface

#### Scenario: Same-session reopen rejects the old modal result
- **WHEN** a nonzero-nonce context-info request completes after the usage modal is closed and reopened for the same session with a new modal nonce, even if the session binding is unchanged
- **THEN** neither live context state nor the reopened modal or scrollback state is changed

### Requirement: Agent communication tools expose their intent and result

通信工具 SHALL 在同一发送工具行显示实际工具身份、参与方、简短状态和有界正文。固定 UI 文案 SHALL 使用英文，消息正文 SHALL 保持原语言。常规列表、展开与正文详情 SHALL NOT 增加 Sideband、主上下文去向或投递模式的解释行；完整身份、原始参数和回执数据 SHALL 在按需详情中可检查。状态 SHALL 来自结构化事实，不通过错误文案猜测投递结果。

#### Scenario: Parent sends queued guidance
- **WHEN** agent 发送消息并收到 durable receipt
- **THEN** 原工具行由 Sending 更新为 Received，显示目标和正文；不另加 ACK 行，不显示已读、已生效或任务完成，也不追加投递机制解释。

#### Scenario: Immediate guidance and uncertain acknowledgement
- **WHEN** 父消息请求安全中断，或已派发的消息因超时、断连或取消无法确认接收
- **THEN** 安全中断参数在按需数据中保留且不推断不可中断工具已停止；无法确认时原行显示 Unconfirmed 和简短原因，不将其当作未送达，不自动重发。

#### Scenario: Parent child or peer inquiry
- **WHEN** ask 得到 Sideband 答案
- **THEN** 原行显示 Answered、问题摘要和答案摘要，接收侧身份准确，完整问答可展开。

#### Scenario: Inquiry state lookup
- **WHEN** get_inquiry 成功查询到失败的 inquiry
- **THEN** 查询成功与 inquiry 失败分别表达，不将两者状态合并。

### Requirement: Parent message receipt appears in its child view

子 agent SHALL 在父消息持久接收后显示一条有稳定 receipt 身份的接收记录，保留来源和原文。常规 UI SHALL 使用英文简洁标题，不强制展示投递模式；原始参数可在按需数据中查看。接收记录 SHALL NOT 冒充人类输入、接收方主动发送工具或额外模型消费。

#### Scenario: Receipt while the child is busy
- **WHEN** child 持久接收消息但尚未消费
- **THEN** 所属视图显示 Message from parent 及正文，不抢焦点，不声称已经执行。

#### Scenario: Duplicate or replayed receipt
- **WHEN** 同一 receipt 在重试、重连或消费后冷恢复中再次投影
- **THEN** UI 保持一条记录，不重复消费或触发副作用。

#### Scenario: Receipt projection fails after commit
- **WHEN** durable commit 成功但 UI 发布失败
- **THEN** 回执仍有效，恢复从持久事实补显示，不重复发送。

#### Scenario: Normal and minimal history
- **WHEN** 在 normal/minimal 或未选中的子视图接收消息
- **THEN** 记录属于正确 Session，Minimal 只打印一次不可变收件事实，主 turn 完成不终止独立 inquiry 行。

### Requirement: Communication presentation preserves readable text

通信记录 SHALL 默认提供有界正文预览，保留可展开、选择和复制的完整文本。工具、参与方、状态与消息正文 SHALL 能在窄终端中辨认，状态不得仅通过颜色区分。

#### Scenario: Long Unicode message containing an image path
- **WHEN** 消息含中文、多行内容、长任务名或本地图片路径
- **THEN** 预览按终端显示宽度换行和明确截断，展开保留原文，图片引用不会替代整条消息文本。

#### Scenario: Keyboard access to communication details
- **WHEN** 用户只使用键盘选择通信记录并进入现有详情入口
- **THEN** 可以阅读和复制完整消息、方向及结果，无需鼠标操作。

### Requirement: Parked foreground waits preserve live control feedback
Pager 在前台等待工具或子 Agent 而采用 parked 显示时，SHALL 继续显示已接收的实时控制反馈，优先于后台任务静态提示；该显示 SHALL NOT 将真实前台改为 Idle、提前应用模型或新增终态事件。终态清除反馈后 SHALL 恢复原等待提示及适用的任务点击区。

#### Scenario: Model selection during a subagent wait
- **WHEN** 前台等待子 Agent，已有待处理模型切换状态
- **THEN** 状态行显示该模型转换信息，切换仍由 Shell 在 Step 边界提交。

#### Scenario: Tool wait with or without background work
- **WHEN** 前台等待工具输出，并收到控制 Pending 或 Applying 反馈
- **THEN** 无论后台任务数量是否为零，都显示反馈，并在窄终端按可用宽度裁剪。

#### Scenario: Control feedback settles
- **WHEN** 权威终态已清除实时反馈而前台仍 parked
- **THEN** 状态行恢复后台任务或 waiting 提示及原有排队、steer 语义，不重复追加完成行。

### Requirement: Hook projections preserve tool ownership across resume

终端 SHALL 使用稳定工具调用身份关联实时与恢复的 pre_tool_use/post_tool_use Hook，保留完成工具及合并 Edit 的归属；SHALL 按 occurrence 身份去重，不能把 Hook 猜测挂到最后一个工具或覆盖其他 occurrence 的详情。

#### Scenario: 对话先恢复而 Hook 后到达
- **WHEN** 完整工具历史已重放，之后收到历史 pre/post tool Hook
- **THEN** Hook 附着到对应工具行，不在尾部逐项创建工具 Hook 行。

#### Scenario: 并行工具与合并 Edit
- **WHEN** 两个工具完成顺序不同，或同文件 Edit 已合并
- **THEN** 每个 Hook 仍属于包含其调用的行，同 phase 的多个 occurrence 均保留。

#### Scenario: 重复快照
- **WHEN** 相同 occurrence 在实时与多次恢复快照中出现
- **THEN** 同一视图只展示一次该 occurrence。

#### Scenario: 实时 Hook 先于工具行到达
- **WHEN** 实时 pre/post Hook 早于相应 ACP ToolCall 到达
- **THEN** 按工具身份暂存并在工具行到达时附着；若该工具隐藏或回合结束仍无锚点，则保留显式生命周期记录，不错挂其他工具。

证据入口：`crates/codegen/pager/src/acp/tracker.rs`、`crates/codegen/pager/src/app/acp_handler/session_notification.rs`。

### Requirement: Unanchored historical hooks have a compact inspectable presentation

终端 SHALL 显式识别历史 Hook 快照；没有对应 pre/post 展示位置的历史生命周期 Hook、无可靠工具锚点的 Hook 与说明 SHALL 合并到每视图一个默认折叠的历史条目，保留身份、事件及运行结果。历史 stop SHALL 不影响当前 turn 的待挂接 Hook。根与子会话视图 SHALL 使用相同行为。

#### Scenario: 恢复大量生命周期与隐藏工具 Hook
- **WHEN** 恢复包含大量 notification、session、stop 或没有展示工具行的 Hook
- **THEN** 它们集中在一个可展开历史条目，失败与阻止详情仍可检查。

#### Scenario: 加载结束后才收到快照
- **WHEN** loading 已结束且另一个 turn 正在运行，历史快照才到达
- **THEN** 仍按历史处理，不加入当前 stop stash，也不生成逐项实时通知。

#### Scenario: 加载期间收到实时执行
- **WHEN** resident actor 在客户端加载期间产生新的实时工具 Hook
- **THEN** 按实时身份关联或暂存，不因 loading 状态误认成历史快照。

证据入口：`crates/codegen/pager/src/scrollback/blocks/tool/lifecycle.rs`、`crates/codegen/pager/src/app/acp_handler/session_notification.rs`。

### Requirement: Forked response history belongs to the new lineage

普通 fork SHALL 在新 Timeline lineage 中保留已继承的响应展示。父 response projection SHALL 经父 Timeline 核对后转换成继承历史，不能把父 admission identity 作为子 Timeline authority；恢复 SHALL 不重新采样或执行工具。

#### Scenario: Fork inherits projected text and reasoning

- **WHEN** 普通 fork 继承父会话的 admitted text、reasoning 及其后工具历史
- **THEN** fork 的 typed/raw/direct replay 各显示一份有序历史，且不携带父 admission/candidate 身份。

#### Scenario: Fork excludes discarded response history

- **WHEN** 父会话含 rewound、quarantined response 或 fork 指定截断点
- **THEN** 子会话只继承所选历史，不复活被排除的 raw candidate；fork_filter 继续清除展示缓存。

### Requirement: Resident replay preserves the physical snapshot frontier

Resident reconnect SHALL 不越过尚未由其 Timeline snapshot 覆盖的 response projection 后再丢弃该记录。初次回放和后续增量 SHALL 共享准确的物理截点，保持 response 和后继更新的顺序及恰好一次交付。

#### Scenario: Response completes between authority and cache snapshots

- **WHEN** resident actor 在 load 取得 Timeline snapshot 之后、读取 updates snapshot 之前提交新 response projection
- **THEN** 初次回放在该物理记录之前截止，delta 在 load 完成前交付该响应和后继工具一次。

#### Scenario: Already synthesized response reaches the physical cache

- **WHEN** Timeline snapshot 已包含 response，初次回放合成它，而物理 projection 稍后追加
- **THEN** delta 不重复展示该 response，后继独立更新仍交付。

### Requirement: Communication bodies support Markdown and lossless source access

通信记录的消息、问题与回答 SHALL 支持正文 Markdown 展示，并与方向、身份、状态和错误等界面字段分开渲染。折叠记录 SHALL 按状态提供以显示宽度计算的有界预览：等待询问最多两行问题，已回答最多一行问题加两行答案，询问失败最多一行问题加两行原因，父消息最多两行消息；投递未知 SHALL 保留原因预览。展开正文和详情 SHALL 保留全文浏览、选择及原始 Markdown 复制入口，原始协议数据 SHALL 可单独查看，默认正文 SHALL NOT 重复附上整份 JSON。查看模式 SHALL NOT 改写存储原文、模型输入或原始工具结果。

#### Scenario: Answer becomes visible without opening details
- **WHEN** 等待中的询问完成并返回答案
- **THEN** 同一默认行保留问题摘要并展示答案摘要；预览直接派生自已有正文，不启动额外模型总结，失败时在同一位置显示实际原因。

#### Scenario: Markdown question and answer
- **WHEN** 询问的问题或回答含标题、列表、引用、行内代码、代码块、链接或表格
- **THEN** 详情将问题与回答分为独立正文区域并使用现有 Markdown 能力展示，原文视图和复制保留 Markdown 源文本及代码缩进。

#### Scenario: Exact source differs from rendered Markdown
- **WHEN** 问题、答案或消息含 tab、CRLF、soft break、围栏或复杂链接语法
- **THEN** 原文复制保留实际接收的正文字符串，不从展开 tab、合并 soft break 或添加元信息的渲染结果重建。

#### Scenario: Parent message and incoming inquiry use the same body behavior
- **WHEN** 同样的 Markdown 出现在父消息接收通知、询问发送行或询问接收行
- **THEN** 三个入口提供一致的正文展示、原文访问和复制行为，图片路径或 Markdown 图片引用不替代整条文字记录。

#### Scenario: Body cannot override communication metadata
- **WHEN** 正文含看似“已接收”“来自父 Agent”的标题，或任务名含 Markdown 控制字符
- **THEN** 界面元信息仍由结构化事实产生并与正文分区，正文不改变真实参与方、交互模式和状态。

#### Scenario: Full source after clipping and resize
- **WHEN** 中文、emoji、长代码行或表格在小宽度下被预览截断并随后调整终端宽度
- **THEN** 正文重新排版，截断有明确标记，全文和复制不丢失；两个不同正文区之间不发生代码围栏或表格结构串扰。

#### Scenario: Replay and minimal rendering
- **WHEN** 通信记录通过 normal、minimal 或重连回放展示
- **THEN** 各入口保持相同语义及原文，Minimal 仍只在询问自身终态后追加接收行，父消息不可变收件通知仍仅追加一次，显示切换不触发新的消费或 Hook。

#### Scenario: Keyboard reading preserves established navigation
- **WHEN** 用户从选中的通信记录进入详情，切换正文/原文/数据并返回
- **THEN** 现有展开、详情、搜索、选择、换行和关闭操作保持可用，输入状态不误触查看动作，Tab 不被改成详情分页；退出恢复原记录的焦点和位置。

#### Scenario: Result arrives during reading
- **WHEN** 新的终态内容到达而用户已滚动离开底部或正在选择正文
- **THEN** 状态更新仍关联原记录，不抢焦点或跳到其他 Session，不以新内容替换正在复制的选区；显示更新后仍可查看最新完整结果。

### Requirement: Accepted response history is rebuilt from Timeline authority

每个带 response admission identity 的 canonical assistant response SHALL 具有 versioned、可校验、可重建的 replay projection。`updates.jsonl` SHALL 只保存该 projection 的 cache record，不得把 provisional provider chunks 或 public SamplingAttempt lifecycle 当作第二份 response authority。所有 production replay 入口 SHALL 在建立 replay snapshot、cursor cutoff 或释放 buffered live events前，以 Timeline identity、event 与 digest 校验并补齐缺失 projection。

#### Scenario: Process stops after Timeline admission

- **WHEN** Timeline 已 durable 接纳 response，但进程在 projection record 提交或 public Accepted 前停止
- **THEN** cold load 从同一 Timeline response 合成并展示恰好一份 accepted history；不得调用 provider、继续 truncation/pause-turn 或执行工具。

#### Scenario: Projection append acknowledgment is lost

- **WHEN** projection record 可能已提交但 ACK 丢失
- **THEN** 系统按 response identity、Timeline event、digest 和 projection version exact reconcile；同内容不重复，不同内容 typed conflict 并 fail closed。

#### Scenario: Candidate output precedes durable projection

- **WHEN** text、reasoning 或 fallback candidate 已向支持撤回的 live client 发布，但 projection durable ACK 尚未返回
- **THEN** candidate 仍可撤回且不作为 accepted cache；ACK 前不发布 Accepted，不开始后续 provider request、工具或 Turn success。

#### Scenario: Quarantined response is replayed

- **WHEN** Timeline admission 的 deterministic quarantine result 非零
- **THEN** projection 记录已处理/Discarded disposition，不把 raw malformed tool preview 重建成 accepted 或 executable history；现有安全 repair/diagnostic 语义保持。

#### Scenario: Tool-bearing response has later tool history

- **WHEN** accepted response 包含工具调用且 cache 中存在后续 ToolCall/ToolCallUpdate 或 tool result 展示
- **THEN** response projection boundary 在这些记录之前恢复，reconciliation 不重新 dispatch 工具，既有工具结果只显示一次。

#### Scenario: Earlier attempt was discarded

- **WHEN** 同一 request 的较早 attempt 只有 transient candidate，而较后 attempt 具有 Timeline response admission
- **THEN** 只按 exact `{request_id, attempt}` 重建较后 response；较早 candidate 不进入 replay。

#### Scenario: Rewind or legacy response

- **WHEN** identity-bearing response 已被 rewind 切出当前 branch，或历史 response 没有 admission identity
- **THEN** reconciliation 不复活 rewound response，也不通过文本、位置或邻近 request 猜测 legacy projection。

#### Scenario: Direct replay without writer authority

- **WHEN** 子任务视图、导出或其他 read-only production reader 读取存在缺失 projection 的会话
- **THEN** 使用同一 Timeline-derived projector 在内存中返回完整去重历史，不获取 writer lease或修改持久数据。

#### Scenario: Cursor intersects one projection record

- **WHEN** reconnect cursor 位于一个可展开为多条 ACP update 的 response projection 内部，且无法证明其余 update 已应用
- **THEN** replay 回退为完整历史替换，不跳过半个 response，也不在 later event 之后补发缺失前件。

### Requirement: Minimal native scrollback preserves semantic lines and links

Grow minimal 模式将稳定条目提交到原生终端 scrollback 时 SHALL 以本次渲染的源行 provenance 区分软接续和硬换行，去除无语义的布局尾部填充，并保留已在 `BlockLine.content` 中的源末尾空格、可见文本、必要背景、宽字和 OSC 8 链接目标。生产者在生成 `BlockLine` 前已丢弃的源空白不属于可恢复范围。每个可见视图 epoch 的提交 SHALL 遵守 print-once frontier：终端写入报告成功后才标记条目已提交；失败时条目保持 live、布局重新测量。切换 root/child 视图后，当前 epoch 可重新打印该视图的已保留历史，旧 epoch 的打印仍保留在终端原生历史。不承诺部分终端写入后的重试恰好一次。

#### Scenario: Short structured text has no copied layout padding
- **WHEN** 短 YAML/Markdown 条目比终端宽度短并被提交
- **THEN** 原生复制内容不包含为布局填充的右侧空格或额外空行，必要的源空白仍保留。

#### Scenario: Long logical token wraps naturally
- **WHEN** 长路径或链接所在的下一渲染行由空 joiner 软接续、无重复可见装饰前缀，且前一行确实满宽
- **THEN** 终端在原生 scrollback 产生软折行，复制可得到未插入换行或补空格的原 token，即使折链跨过内部屏高分块。

#### Scenario: Decorated continuation cannot be losslessly native-wrapped
- **WHEN** 空 joiner 的续行重复绘制引用竖线、编辑路径缩进或其他不可选择的可见前缀
- **THEN** 提交保留该视觉行但保守使用硬换行，不把装饰前缀伪装成原 token 的无缝续接；该类复制不会承诺还原完整逻辑 token。

#### Scenario: Full-width source hard break is not joined
- **WHEN** 代码行刚好满宽但下一行来自硬换行，或 joiner 为一个空格/换行
- **THEN** 后一行不与前一行作为无分隔软接续，硬换行及源分隔语义保持。

#### Scenario: Wide glyph and truncation footer
- **WHEN** 行含 CJK/emoji 宽字或提交高度上限产生 footer
- **THEN** 宽字仅输出一次、列位置准确；footer 与前一行硬隔离，不继承被覆盖行的软折标记或链接。

#### Scenario: Linked text crosses a row boundary
- **WHEN** OSC 8 链接跨软折行、在边界结束或后面紧跟无链接文本
- **THEN** 终端可见字形与完整 URL/id 对齐，链接及时关闭且不泄漏到后续文字；pending wrap 不因中间控制序列丢失。

#### Scenario: Commit meets resize or writer failure
- **WHEN** 本次提交过程中终端尺寸改变，或 native writer 报错
- **THEN** 已开始的条目以一致的起始宽度计算行语义；失败条目不越过 frontier 且下一帧 live tail 重测，viewport/prompt 不多滚或跳行。

### Requirement: Kitty keyboard teardown has an owned reply fence

Grow 在正常终端退出中 SHALL 先确认输入 reader 不再消费 TTY，并收束已接受的渲染输出，然后发出已推入 Kitty keyboard flags 的 pop。仅当确实推入过 flags、输出可写且当前进程独占 TTY 输入时，SHALL 在仍为 raw mode 的状态下向该终端发送 DA1 查询，有界消费输入直至收到完整 DA1 回复，再关闭 raw mode。fence 的超时或 I/O 错误 SHALL 不阻止 best-effort 终端恢复；不得通过与存活 reader 并发读取来假装完成屏障。

#### Scenario: Late Kitty release on normal quit
- **WHEN** flags 已推入、用户正常退出，终端在旧 10 ms drain 窗口之后、DA1 回复之前发送 keyboard release
- **THEN** writer 帧先于 teardown，pop 先于 DA1；fence 消费 release 并在完整 DA1 后恢复 raw mode，release 不留给 shell。

#### Scenario: Silent or malformed DA1
- **WHEN** 终端不回复、只回复部分序列，或回复 DA2/CSI-u 而非完整 DA1
- **THEN** 不将其判为 fence 完成；到固定有限期限后仍关闭 raw mode、显示光标并返回，不无限等待。

#### Scenario: No Kitty flags
- **WHEN** 会话未成功推入 Kitty keyboard flags
- **THEN** 退出不发 pop 或 DA1，沿既有恢复路径结束。

#### Scenario: Input or output ownership cannot be established
- **WHEN** reader 停止确认超时、writer 无法安全收束、查询写入失败，或没有可用 TTY
- **THEN** 不与 reader 并发读 stdin，不执行不可靠的回复读取；仍执行可行的终端恢复并记录降级原因，退出不因 fence 增加无界等待。

#### Scenario: Forced exit remains fast
- **WHEN** 首次受控退出信号转为正常 quit，或 panic/第二次信号走强制退出
- **THEN** 前者在条件满足时经过正常 fence；后者只做快速 best-effort teardown，不等待 reader、writer 或 DA1。

### Requirement: Slash MRU snapshot writes obey the encoded input allowance
Slash MRU persistence SHALL reject encoded snapshots larger than 1,048,576 bytes before background handoff and before filesystem publication. Rejected snapshots SHALL leave the destination unchanged; a rejected handoff SHALL be reported to the controller so its store remains dirty for retry. Snapshots at or below the allowance SHALL retain atomic publication behavior.

#### Scenario: Snapshot fits the encoded allowance
- **WHEN** a serialized Slash MRU snapshot is at most 1,048,576 bytes
- **THEN** it may be handed to the background writer and is published as a complete atomic replacement

#### Scenario: Snapshot exceeds the encoded allowance
- **WHEN** a serialized Slash MRU snapshot is larger than 1,048,576 bytes
- **THEN** background handoff and filesystem publication reject it, preserve any existing destination, and the controller retains dirty state after the handoff failure

Evidence: `crates/codegen/pager/src/slash/mru.rs` (`MAX_STORE_BYTES`, `MruSnapshot::write`, `persist_async`, `snapshot_write_enforces_the_reader_byte_allowance_before_publication`) and `crates/codegen/pager/src/slash/mod.rs` (`SlashController::record_command_use`, exercised by `controller_rejects_oversized_snapshot_and_retains_dirty_state`).

### Requirement: Incomplete search bootstrap preserves index state
Session search bootstrap SHALL treat a missing or invalid summary inside an opened session directory, or a failure in any required Timeline read or index write, as an incomplete attempt. An incomplete attempt SHALL NOT prune existing index rows or publish a completed-bootstrap marker. After the source is repaired, a bootstrap recheck SHALL be able to rebuild the affected index row. Evidence: `crates/codegen/shell/src/session/storage/search.rs` (`reindex_all`, `SearchIndexJob::RecheckBootstrap`) and `crates/codegen/shell/src/session/storage/jsonl/mod.rs` (session summary enumeration).

#### Scenario: Missing or invalid summary
- **WHEN** bootstrap opens a session directory whose summary is missing, cannot be decoded, or cannot be validated
- **THEN** it fails the attempt before orphan pruning, preserves existing indexed rows, and leaves no completed-bootstrap marker

#### Scenario: Required Timeline or index operation fails
- **WHEN** a session's required Timeline read or index write fails during bootstrap
- **THEN** the attempt remains incomplete, existing index rows are not pruned, and no completed-bootstrap marker is published

#### Scenario: Recheck after source repair
- **WHEN** a failed bootstrap is followed by repair of the invalid summary or Timeline and a bootstrap recheck
- **THEN** the recheck indexes the repaired session and publishes the completed-bootstrap marker

### Requirement: Automatic recap is suppressed during reconnect
Pager SHALL NOT dispatch automatic recap requests while reconnect initialization or session reload is pending. Suppressed poll attempts SHALL remain no-ops and SHALL NOT record automatic retry backoff. A focus-return event during reconnect SHALL drop that away-period recap opportunity under the existing best-effort behavior. After reconnect completes, recap availability SHALL follow the replacement shell's refreshed capability; manual requests and a later away period SHALL use the normal admission path.

#### Scenario: Replacement disables recap while session reload is delayed
- **WHEN** the previous shell advertised recap, a replacement shell disables it, session reload is still pending, and the automatic recap poll fires
- **THEN** Pager sends no recap request and records no automatic retry attempt; after reload completes, the refreshed disabled capability remains authoritative

#### Scenario: Focus returns during reconnect
- **WHEN** focus returns after the recap threshold while reconnect is pending
- **THEN** Pager sends no automatic recap request and drops only that away-period opportunity; after reconnect, manual requests and a later away period follow the refreshed capability normally

### Requirement: Registered tool calls retain visible identity and terminal state

Shell SHALL give every registered tool input an explicit ACP start presentation and every tool output an explicit ACP projection with the original tool-call identity. Successful completed outputs SHALL close their rows; a backgrounded Bash output MAY remain in progress until its task finishes. Adding a new closed tool input or output variant SHALL require an explicit projection decision instead of silently falling through a wildcard.

#### Scenario: LSP and dynamic tool starts
- **WHEN** an LSP call or a dynamically registered tool starts
- **THEN** the Pager receives a start update identifying the actual operation or tool name, with the original raw input; neither starts as an anonymous `Tool call`.

#### Scenario: Context recall and dynamic tool results
- **WHEN** ContextRecall or a dynamic tool returns successfully
- **THEN** the same tool-call ID receives a Completed update with its result evidence, so the row stops running before the model turn ends.

### Requirement: Unified log appends coordinate with in-place trimming
Unified log writers SHALL acquire the opened log inode's exclusive advisory lock before appending a complete encoded entry, using the same lock that guards in-place trim rewrite and truncate. A trim encountering an active append SHALL yield under its existing nonblocking lock policy. An append encountering an active trim SHALL wait until that trim releases the lock before writing, so a successfully written line is not removed by that trim's truncation.

#### Scenario: Trim owns the inode while a writer appends
- **WHEN** trimming has locked the log inode and a writer starts an append before trimming rewrites and truncates it
- **THEN** the append completes after the trim releases the lock and remains after the retained tail

#### Scenario: Append owns the inode while trim is attempted
- **WHEN** a writer holds the inode lock for an append and another process attempts a nonblocking trim
- **THEN** that trim leaves the inode unchanged and may be retried on later maintenance

### Requirement: Unified log records have a bounded complete encoding
Unified log writers SHALL encode each JSONL record, including its terminating LF, within 65,536 bytes. Serialization SHALL stop before appending bytes that exceed this budget. An oversized record SHALL be replaced by a complete, valid diagnostic record identifying the omitted entry and the byte limit; no prefix of the rejected encoding SHALL be written.

#### Scenario: Entry fits the record budget
- **WHEN** a Shell or Pager log entry serializes to at most 65,536 bytes including LF
- **THEN** the writer appends that complete encoded entry unchanged.

#### Scenario: Entry exceeds the record budget
- **WHEN** a Shell or Pager log entry would exceed 65,536 bytes including LF
- **THEN** the writer appends one valid diagnostic JSONL record identifying `record_omitted` and the 65,536-byte limit, with no bytes from the rejected encoding.

#### Scenario: One oversized record has no line boundary
- **WHEN** an entry contains a message or context too large for the record budget
- **THEN** serialization memory remains bounded by the per-record budget and the persisted file receives the bounded diagnostic line with its terminating LF.

### Requirement: Explicit scroll log paths have exclusive writer ownership
A scroll log recorder opening an explicit target SHALL acquire a nonblocking exclusive advisory lock on the opened regular file before truncating it, and SHALL retain ownership while writing. If ownership is unavailable, the recorder SHALL disable itself without modifying the existing file. The existing non-regular target rejection and per-recorder byte limit SHALL remain in effect.

#### Scenario: Second recorder targets the same file
- **WHEN** one recorder owns an explicit scroll log path and a second recorder attempts to open that same file, including through a path alias resolving to the same file
- **THEN** the second recorder disables itself without truncating or writing, and the first recorder's complete records remain intact

#### Scenario: Ownership is released
- **WHEN** the owning recorder is dropped and another recorder opens the target
- **THEN** the later recorder can acquire ownership and begin a new recording using the existing truncate-on-first-record behavior

### Requirement: Child transcript file notices retain the export origin

An explicit `/export` or `/copy` file job started from a child Agent view SHALL snapshot that child's content, cwd, Agent identity and session identity. Admission and completion notices SHALL appear in that same child view even if the user switches to the parent before the job completes. A removed or rebound child SHALL NOT receive the old job's feedback; the file queue SHALL still advance.

#### Scenario: Minimal child export completes after a view switch

- **WHEN** `/export` is submitted from a Minimal child view whose content and cwd differ from its parent, then the user switches to the parent before the write completes
- **THEN** the file contains the child's transcript at the child's resolved path, the child receives saving and completion notices, and the parent receives neither notice

#### Scenario: Child is removed or rebound before completion

- **WHEN** the child view that submitted a file job is removed or bound to another session before the result arrives
- **THEN** the file job settles and the queue advances without publishing the old completion to a different view or session

### Requirement: Minimal transcripts retain the selected view owner

Minimal `/transcript` SHALL capture the selected root or child Agent view and session identity when invoked. Its incremental rendering, cwd/media resolution, external pager handoff and failure notices SHALL use that captured view across focus changes. A removed or rebound child SHALL NOT be replaced by a parent or another child with matching entry IDs.

#### Scenario: Child transcript survives a focus switch

- **WHEN** `/transcript` begins in a Minimal child view with content and cwd distinct from the parent, and focus returns to the parent during the build
- **THEN** every slice and the pager file use the child content and cwd; pager failure feedback targets the same child

#### Scenario: Captured child is removed or rebound

- **WHEN** the child view disappears or binds to a different session before the build finishes
- **THEN** the in-flight build is dropped without opening a pager or publishing feedback to another conversation

#### Scenario: Owner reload during a build

- **WHEN** the captured view enters session reload while its transcript is still being rendered
- **THEN** the old prefix is discarded and the build waits for that view's final state before restarting

### Requirement: Minimal transcript snapshots write outside the interactive loop
Minimal 全文 transcript 的渲染 SHALL 继续按每帧 8ms 预算分片；渲染完成后的 ANSI 临时快照文件 I/O SHALL 在后台阻塞任务执行，避免最终写入占用 UI 事件循环。

#### Scenario: Snapshot write is slow
- **WHEN** Minimal transcript 的完整 ANSI 正文已渲染且快照写入尚未完成
- **THEN** UI 事件循环继续处理输入和绘制；分页器请求只在完整私有快照写入成功后提交。

#### Scenario: A newer request replaces an in-flight write
- **WHEN** 旧 transcript 快照仍在后台写入时产生更新的 Minimal transcript 请求
- **THEN** 旧结果完成后被释放，不启动旧分页器；仅当前请求的完整快照可交给分页器。

#### Scenario: Original owner disappears or changes binding
- **WHEN** 快照写入完成时原 root、选中 child view 或 session identity 已不存在或不匹配
- **THEN** 丢弃并清理该快照，不将文件或错误反馈交给当前其他会话。

#### Scenario: Snapshot write fails
- **WHEN** 后台创建或写入 ANSI 快照失败
- **THEN** 清理未完成的私有临时文件，并将错误反馈给仍匹配的原 owner；UI 事件循环不等待磁盘操作。

### Requirement: Usage surfaces include auxiliary model consumption

`/usage` SHALL display the owning session's known main, child and Sideband model consumption in lifetime and resume segments, grouped by the frozen provider/model route. Full input plus output includes cache-hit input. Incomplete Sideband attempts SHALL preserve the lower-bound marker and prevent unknown cost from appearing exact. Sideband calls SHALL NOT increment the public main-loop turn count. Existing aggregate UI and headless usage shapes remain unchanged.

#### Scenario: Auxiliary calls across a resume

- **WHEN** a session's main loop, recap and memory Sidebands consume known tokens across two incarnations
- **THEN** `/usage` and the ordinary status projection include all three charges once, each in its proper segment and route group, while `numTurns` counts only main-loop rounds

#### Scenario: Auxiliary call with unknown usage

- **WHEN** a Sideband provider request completes or fails without trustworthy usage
- **THEN** `/usage` and headless reporting show recorded totals as an incomplete lower bound and do not present unknown cost as exact

### Requirement: Agent-local tasks end with their owner
Local background tasks that access an `MvpAgent` SHALL stop before the agent is destroyed, even when the agent's owning task is aborted while its `LocalSet` remains active.

#### Scenario: Abrupt owner task abort
- **WHEN** the task owning a leader agent is aborted with its local session supervisor and coordination work pending
- **THEN** those local tasks are cancelled before the agent's state is destroyed and cannot execute a later tick against the old agent.

### Requirement: Session picker list results belong to the latest visible fetch
Pager SHALL apply a session picker list result only when it belongs to the latest list fetch and a picker surface is still visible. Dismissal SHALL invalidate in-flight list results.

#### Scenario: Rapid successive fetches
- **WHEN** two list requests are issued and the older request completes last
- **THEN** only the newer result remains visible.

#### Scenario: Modal closes before a response
- **WHEN** an Agent session picker modal is dismissed while its list request is pending
- **THEN** the late response does not repopulate the closed modal or the welcome picker.

#### Scenario: Welcome closes before a response
- **WHEN** the welcome picker is dismissed while its list request is pending
- **THEN** the late response does not restore its entries or loading state.

### Requirement: Session picker list scope follows the visible owner
Pager SHALL request an Agent picker list using that visible Agent or child session's cwd. A list result SHALL update only the same visible picker and session binding that requested it. Relaxed-scope notices SHALL use the request cwd.

#### Scenario: Agent cwd differs from launch cwd
- **WHEN** a picker opens in an Agent whose session cwd differs from the app launch cwd
- **THEN** the list request uses the Agent session cwd, and the result's selection anchor and relaxed-scope notice use that same cwd.

#### Scenario: View or session binding changes before result
- **WHEN** the user switches Agent or child view, changes cwd, or rebinds the session while a list fetch is pending
- **THEN** the old result does not update the new visible picker, loading state, or toast.

### Requirement: Pending session pickers recover after owner switches
When a visible session picker remains loading after another view supersedes its list request, Pager SHALL issue a new request for the visible picker. A late result from the former owner SHALL NOT update the new picker.

#### Scenario: Switch between pending Agent pickers
- **WHEN** Agent A and Agent B have pending picker modals and focus switches between them
- **THEN** each newly visible pending modal receives a fresh list request bound to its own cwd; late results from the other modal do not replace its entries.

#### Scenario: Switch to a loaded picker
- **WHEN** focus moves to a picker whose list has already loaded
- **THEN** its entries remain available without an unnecessary refetch.

### Requirement: Agent teardown retires its session writers
When an `MvpAgent` is destroyed while its process runtime continues, it SHALL request shutdown of every primary and active child session actor it owns. A replacement agent SHALL not load the same session until the old writer has released its lease.

#### Scenario: Agent owner ends with resident sessions
- **WHEN** the Agent owner is dropped while primary or child session actors remain resident
- **THEN** each actor receives the existing shutdown command and the owner releases its handles without blocking the local event loop.

#### Scenario: In-process leader replacement
- **WHEN** a leader generation is stopped and a replacement generation will load the same session
- **THEN** the replacement waits until the old session writer lease is released before loading; it does not overlap writers or treat an active lease as a successful load.

### Requirement: List layout queries respect cached item bounds
The Pager list layout cache SHALL report per-item geometry only for indexes represented by the cache, and SHALL support appending items to either fixed-height or variable-height layouts without panicking.

#### Scenario: Query outside an empty or populated cache
- **WHEN** a caller requests an item's virtual y or height at an index greater than or equal to the cached item count
- **THEN** the query returns no geometry instead of fabricating a position or height.

#### Scenario: Append to either layout representation
- **WHEN** incremental item heights are appended to a fixed-height cache
- **THEN** the cache count grows by the number of appended items and each appended item has height one.
- **WHEN** incremental item heights are appended to a variable-height cache
- **THEN** each height and its corresponding prefix sum are added consistently.

### Requirement: Scrollback search retains bounded pending work
Scrollback search SHALL retain at most one pending merged request while a worker scans and SHALL submit query updates without waiting for worker capacity. Its result identity SHALL continue to prevent an older scan from replacing the visible query result.

#### Scenario: Query burst during a scan
- **WHEN** a new corpus and several newer queries arrive before the worker can take pending work
- **THEN** the pending request retains the newest corpus and query, submission does not wait for scanning, and only the latest matching result is applied to the current search view.

#### Scenario: Search owner closes with pending work
- **WHEN** search closes while a scan or pending request exists
- **THEN** stop takes precedence over the pending request without waiting for worker notification capacity; the worker does not apply that pending request after closing.

### Requirement: File search results belong to the current query
Pager file search SHALL expose results only when they belong to the currently active query request. Submitting a new query SHALL immediately make prior results unavailable for display and selection.

#### Scenario: Previous query completes after dismiss and reopen
- **WHEN** a user dismisses file search, opens it again with another query, and the daemon publishes a snapshot from the previous query before processing the new request
- **THEN** the previous snapshot is not displayed or selectable, and results from the current request can be applied

#### Scenario: Previous query remains visible during an edit
- **WHEN** the active `@` query changes while an earlier daemon snapshot is still available
- **THEN** the old snapshot is cleared immediately and cannot be restored by a late poll

证据：`crates/codegen/pager/src/views/file_search/state.rs` — `start_query`, `poll`, `apply_results`；`crates/codegen/workspace/src/file_system/fuzzy.rs` — `FuzzyFileMatcherDaemon::set_query` 与结果快照的 `query_id`。

### Requirement: Fuzzy file-search submissions and teardown do not wait for worker capacity
Fuzzy file search SHALL submit the latest restart and query without waiting for the worker's notification channel or an ongoing filesystem walk. Closing its daemon SHALL not join the worker on the caller's thread and SHALL prevent pending requests from running after stop is observed.

#### Scenario: Slow walk with rapid input
- **WHEN** a worker is occupied while multiple restart and query updates arrive
- **THEN** submission remains nonblocking, only the newest pending restart and query remain, and the published result retains that query's request identity.

#### Scenario: Coalesced query and workspace status polling
- **WHEN** workspace fuzzy-search status polling starts after several query changes were coalesced by the worker
- **THEN** it waits for the latest query identity, does not publish an older query's result, and can publish the latest result without waiting for skipped worker generations.

#### Scenario: Search closes during worker activity
- **WHEN** the daemon is dropped with pending work or an active walk
- **THEN** stop supersedes pending work, the current walk is asked to cancel, and the caller returns without waiting for channel capacity or worker join.

### Requirement: macOS clipboard text subprocesses have bounded execution
macOS `pbpaste -Prefer txt` 文本读取 SHALL 使用有界子进程运行器：执行时间最多 5 秒，stdout 和 stderr 各最多 1 MiB，并在调用结束时回收其所属进程组；超时、任一流超限、启动或非零退出 SHALL 返回读取错误。

#### Scenario: Text command hangs or exceeds an output allowance
- **WHEN** `pbpaste` 超过 5 秒，或 stdout/stderr 任一流超过 1 MiB
- **THEN** 读取失败且所属进程组被回收，不继续无限等待或收集输出。

#### Scenario: Text command succeeds within budget
- **WHEN** `pbpaste` 在期限内以零状态退出且两条流均在限额内
- **THEN** 保留原有语义：空 stdout 返回无文本，非空 stdout 按 UTF-8 有损解码返回文本。

### Requirement: Invalid passive inquiry identities remain finite notices
Pager SHALL coalesce an incoming coordination notice into a passive inquiry row only when its structured audit and non-empty correlation ID provide a valid identity. A notice without valid identity SHALL remain visible as a finite raw notice and SHALL NOT enter passive running lifecycle. The scrollback upsert SHALL reject absent or empty identity without panicking or mutating state.

#### Scenario: Incoming notice has no structured audit or usable inquiry ID
- **WHEN** an incoming inquiry has missing or malformed structured audit data, or an empty correlation ID
- **THEN** Pager retains the original notice as a finite row and creates no passive coordination entry.

#### Scenario: Internal upsert receives absent or empty identity
- **WHEN** `upsert_coordination_row` receives a block without a coordination identity, or with empty source peer or inquiry ID
- **THEN** it returns without mutation and does not panic or create a running row.

#### Scenario: Valid inquiry identity
- **WHEN** a non-empty source peer ID and correlation ID accompany a valid audit
- **THEN** the existing correlated passive row behavior and running/terminal projection remain unchanged.

### Requirement: Flowchart class annotations preserve node meaning

Grow 的 Mermaid flowchart 静态预览 SHALL 在识别节点尾部 `:::class` 时保留节点原有 ID、形状和标签，并将已定义 `classDef` 的 `fill`、`stroke`、`color` 分别用于节点填充、边框和文字。节点的显式 `style` SHALL 优先于 class 样式；无法静态执行的交互类指令 SHALL 不变成伪节点。本要求只覆盖 `classDef` 与内联 `:::` 子集，不承诺完整 Mermaid class 语法。

#### Scenario: 带形状和 API 路径的 class 节点
- **WHEN** flowchart 包含 `PIN{machine claimed?}:::cond` 和 `CALL["POST /v1/items/{id}"]:::dark`，并定义对应 `classDef`
- **THEN** 预览保留菱形、矩形及完整标签，已定义的填充、边框和文字颜色体现在 SVG/PNG 中，不以 ID 或被截断的标签替代。

#### Scenario: 显式 style 与 class 并存
- **WHEN** 同一节点同时引用 `classDef` 并拥有显式 `style` 语句
- **THEN** 显式 `style` 继续决定该节点的有效样式，class 不在解析结束时覆盖它。

#### Scenario: 不支持的静态指令
- **WHEN** flowchart 含 `class`、`click`、`linkStyle`、`direction`、`accTitle` 或 `accDescr` 指令
- **THEN** 静态预览不将这些整行指令画成节点；本要求不意味着指令的交互、链接或方向语义已经实现。

#### Scenario: 样式值包含 SVG 属性定界字符
- **WHEN** `classDef` 或既有 `style` 的颜色值含引号、尖括号或 `&`
- **THEN** 输出 SVG 的颜色属性保持 XML 结构完整，不因该值新增属性或元素；有效普通颜色仍按原值显示。

目标实现入口：`third_party/mermaid-to-svg/src/parser.rs` — `parse_statements`、`try_parse_node`；`src/layout.rs` — `get_node_colors`；`src/svg_renderer.rs` — `render_text_lines` 与各节点形状渲染器。

### Requirement: Flowchart ampersand groups represent individual edges

Grow 的 Mermaid flowchart 静态预览 SHALL 把形状与引号之外、两侧有空白的 `&` 解释为节点组分隔符，将每对相邻组展开为笛卡尔积边；形状或引号内的 `&` SHALL 保留为标签内容，不得生成包含整组文本的伪节点。

#### Scenario: 分组连接和边链
- **WHEN** 输入为 `A & B --> C --> D & E`
- **THEN** 图中有 `A→C`、`B→C`、`C→D`、`C→E` 四条逻辑边，节点 ID 不包含 `A & B` 或 `D & E`。

#### Scenario: 标签中的字面 ampersand
- **WHEN** 输入含 `REP["GET /v1/items?sku=&status=READY"]:::dark`
- **THEN** 标签中的 `&` 保留，不拆成多个节点或边。

目标实现入口：`third_party/mermaid-to-svg/src/parser.rs` — `parse_edge_chain`、节点解析；`crates/codegen/mermaid/src/pure.rs` — `PureRustEngine::render`。

### Requirement: Flowchart group expansion is bounded before allocation

单个 flowchart 中由 `&` 节点组产生的逻辑边 SHALL 至多为 4096 条。解析器 SHALL 在构造超额边之前拒绝输入，并返回可由现有 Mermaid 错误路径处理的解析失败；失败 SHALL 不发布部分 SVG 或 PNG。

#### Scenario: 刚好达到预算
- **WHEN** 分组语句累计恰好展开 4096 条逻辑边
- **THEN** 解析阶段接受该展开，不因边数预算单独拒绝；后续布局和像素限制仍独立适用。

#### Scenario: 超过预算或乘积溢出
- **WHEN** 新组连接会使累计展开边数超过 4096，或组大小乘积无法安全计算
- **THEN** 在构造该组边之前返回解析失败；Pager 走现有渲染失败路径，不展示不完整图片。

目标实现入口：`third_party/mermaid-to-svg/src/parser.rs` — `Parser::parse_edge_chain`；`crates/codegen/pager/src/app/agent_view/mermaid_worker.rs` — 渲染失败处理。

### Requirement: Goal Active queued inputs are directly editable and retractable

Pager SHALL let the user edit or remove a still-pending queued message while Goal is Active through the queue item's existing edit and cancel actions, without selecting Stop Goal or Stop Turn. It SHALL distinguish a confirmed pending item from an optimistic submission and an already-running item, and SHALL report the Shell's authoritative result instead of treating local UI mutation as success.

#### Scenario: Edit a pending message during Goal execution
- **WHEN** Goal is Active, another turn is running, and the user selects `[edit]` or the queue edit key for a confirmed pending message
- **THEN** Pager enters protected editing only after the Shell confirms the hold; the current turn, subagents, and Goal lifecycle remain unchanged.

#### Scenario: Retract a pending message during Goal execution
- **WHEN** Goal is Active and the user selects `[cancel]` or the queue delete key for a confirmed pending message
- **THEN** successful authoritative removal makes the message disappear and prevents its later execution without opening a Goal／turn stop choice.

#### Scenario: Pending submission or stale row
- **WHEN** the selected row is still an optimistic echo, has already started running, was removed elsewhere, or changed version before an edit or remove request is confirmed
- **THEN** Pager does not claim the operation succeeded; it reconciles with the authoritative queue and preserves any unsaved editing text for recovery or explicit user action.

#### Scenario: Failed edit save
- **WHEN** the Shell rejects or cannot durably admit edited content
- **THEN** Pager keeps the editing draft and shows a failure; it does not silently return to normal composer mode or send the old text as the edit result.

#### Scenario: Turn-stop shortcut remains distinct
- **WHEN** the user invokes Ctrl+C without selecting a queued item action
- **THEN** the existing current-turn／Goal interruption semantics remain unchanged; queued-message withdrawal is not inferred from a turn-stop gesture.

### Requirement: Minimal frames render the selected Agent view

When a root Agent selects a child view and has no root permission pending, Minimal SHALL use that child's prompt, controls, live tail, viewport measurements and native scrollback commit frontier for the whole frame. Root permissions SHALL temporarily retain root ownership. The selected view SHALL remain the input owner and the rendering owner together; a missing or rebound child SHALL NOT cause a child input with root rendering.

#### Scenario: Selected child has different content

- **WHEN** a child with a distinct prompt, running tail and finalized history is selected from a root Agent
- **THEN** Minimal measures, commits and draws the child's content and controls without committing new root entries as the child's history.

#### Scenario: Switch away and return

- **WHEN** Minimal switches between a root and a child whose histories were each already printed in an earlier visible epoch
- **THEN** it clears the old visible screen, reprints the selected view's retained history through that view's commit frontier, and draws its own live prompt; older native terminal scrollback remains historical.

#### Scenario: Root permission interrupts child selection

- **WHEN** a root permission arrives while a child remains selected
- **THEN** input and the entire Minimal frame use the root permission owner until it resolves, then return to the selected child.

#### Scenario: Child disappears or is rebound

- **WHEN** the selected child key no longer resolves to that child view or its session binding changes
- **THEN** Minimal does not reuse the former child's committed frontier or render its content for another owner; a missing child key falls back to root input and rendering.

#### Scenario: Native clear or commit fails during a switch

- **WHEN** the terminal cannot clear for a new owner or a new owner's native block write fails
- **THEN** Minimal does not mark missing content as committed; it retries the failed boundary on a later frame and keeps the failed block in the live tail.

### Requirement: Pinned rewind history reads have bounded and visible failure
Pinned rewind metadata and full-history reads SHALL reject a nonblank record above 64 MiB, a scan above 256 MiB, or more than 50,000 records before accepting a partial history projection. A pinned read SHALL not block the async request executor while scanning the file. Failed or cancelled scans SHALL retain the pinned source and live points for a later retry.

#### Scenario: Oversized pinned record or scan
- **WHEN** a pinned rewind record exceeds 64 MiB, a scan exceeds 256 MiB, or a scan has more than 50,000 records
- **THEN** metadata and full-history reads fail without merging a valid prefix or treating the file as empty.

#### Scenario: Cancelled read
- **WHEN** a metadata or full-history request is cancelled while its blocking scan remains in progress
- **THEN** another scan cannot seek the same pinned source until the worker releases its read ownership, and the source remains available for retry.

#### Scenario: Damaged nested snapshot
- **WHEN** metadata encounters a record whose nested snapshot shape cannot be loaded as a rewind point
- **THEN** the picker request fails instead of claiming that checkpoint has no file changes.

#### Scenario: Picker failure feedback
- **WHEN** metadata scanning fails for the active pinned source
- **THEN** the points request reports an error to the client; the current rewind interaction closes, restores its draft, and shows failure rather than a conversation-only choice.

### Requirement: Rewind execution feedback belongs to its source session binding
The client SHALL carry the source session ID and binding epoch with an executing rewind. A result for the same binding SHALL reconcile its committed success or explicit rejection with that Agent view even when another view is active. A result for an obsolete binding SHALL NOT mutate the replacement transcript, composer, inline editor or rewind overlay; it SHALL visibly report the previous session's confirmed success, explicit rejection or unknown transport outcome. A confirmed success outside its original binding SHALL direct the user to reload that session for an authoritative projection.

#### Scenario: Source binding remains active
- **WHEN** an execution result arrives after the user switches to another view without unbinding its source session
- **THEN** the result reconciles against the owning Agent's unchanged binding.

#### Scenario: Source binding is replaced
- **WHEN** an execution result arrives after its Agent unbinds or binds another session
- **THEN** the replacement view's transcript, overlay and draft remain unchanged and a previous-session notice describes the result.

#### Scenario: Same session ID is rebound
- **WHEN** the source session ID is unbound and later rebound before an execution result arrives
- **THEN** the old binding's result does not mutate the new binding and the user is told to reload or verify the session.

#### Scenario: Session changes during inline resubmit
- **WHEN** a binding change interrupts an executing inline-edit rewind
- **THEN** the old pending resubmit cannot be sent into the replacement binding, the old rewind overlay cannot appear there, and unsent draft/edit text remains in the local composer.

#### Scenario: Execution response is lost
- **WHEN** transport or response parsing fails after an execution was sent
- **THEN** the client reports an unknown outcome and directs verification rather than asserting that no rewind committed.

### Requirement: Textual image placeholders do not load files

Pager and Shell SHALL treat a numbered image placeholder in text as an anchor only. They SHALL NOT infer an image attachment by opening the path embedded in that text. Image bytes SHALL enter through an explicit attachment admission path.

#### Scenario: Placeholder without attachment
- **WHEN** submitted text contains `[Image #N: <path>]` without a corresponding image attachment
- **THEN** the model-visible text retains the numbered anchor without the path, and the path is not opened to synthesize an attachment.

#### Scenario: Placeholder with attachment
- **WHEN** submitted text contains a numbered image placeholder and the client submits image content as an attachment
- **THEN** the numbered anchor and attachment content remain available without re-reading the path from placeholder text.

### Requirement: Passive inquiry rows converge by participant and phase

Pager SHALL correlate an incoming passive inquiry row by structured source peer and inquiry ID. Received, approved and terminal facts SHALL advance that row monotonically without consuming the primary turn's tool state. A lower phase or equal-phase replay SHALL NOT replace a newer live projection; terminal state SHALL remain final. A distinct peer with the same inquiry ID SHALL own a distinct row.

#### Scenario: Approval arrives before an older start
- **WHEN** a valid approval notice is displayed and a delayed start for the same source peer and inquiry ID arrives
- **THEN** the existing row retains approval detail and identity without a second row.

#### Scenario: Completion precedes older notices
- **WHEN** a terminal inquiry outcome arrives before start or approval and those older notices later arrive live or by replay
- **THEN** the terminal row remains final and is not duplicated or restarted.

#### Scenario: Parent and child sources interleave
- **WHEN** distinct source peers deliver notices carrying the same inquiry ID in alternating order
- **THEN** each peer's row advances independently and neither consumes the other's completion.

### Requirement: Pre-session command discovery has a bounded execution boundary

Pre-session non-chat `grow/commands/list` SHALL perform folder-trust resolution and plugin, skill, and workflow discovery in the shared single-permit blocking worker under a five-second deadline that includes waiting for capacity. A timeout SHALL NOT claim to interrupt an already-running filesystem operation. Chat catalog requests and live-session command requests SHALL retain their existing paths without this pre-session discovery.

#### Scenario: Pre-session discovery remains blocked after timeout
- **WHEN** pre-session command discovery remains blocked beyond its deadline and another pre-session listing arrives
- **THEN** the first request returns an RPC error, its worker retains the shared discovery permit until it exits, and the later request cannot start an additional scan beyond the concurrency bound

#### Scenario: Pre-session discovery fails or times out
- **WHEN** the discovery worker fails or the deadline expires
- **THEN** `grow/commands/list` returns an RPC error rather than a successful empty command catalog

#### Scenario: Discovery completes with no commands
- **WHEN** all pre-session scans finish successfully and find no commands
- **THEN** the request returns the existing successful empty catalog shape

#### Scenario: Chat and live-session command paths
- **WHEN** `kind="chat"` or `sessionId` selects its existing early-return branch
- **THEN** the request bypasses pre-session plugin, skill, and workflow discovery and preserves its existing response behavior

### Requirement: Agent configuration modal discovery does not block the event path

Pager SHALL display the agent configuration modal immediately while its filesystem-backed catalog loads in a bounded background worker. A timed-out or failed scan SHALL report failure in the modal without making the UI wait for the underlying filesystem call to finish. A result SHALL apply only to the modal instance that requested it.

#### Scenario: Discovery stalls
- **WHEN** opening the modal starts a filesystem scan that remains blocked
- **THEN** the modal opens and remains dismissible, other UI events remain responsive, and the request eventually reports a load error while the worker keeps its execution permit until exit.

#### Scenario: Modal closes and reopens before an older scan completes
- **WHEN** a first modal instance is closed, another instance opens, and the first scan later returns
- **THEN** the old result does not overwrite the newer modal's state.

#### Scenario: Configuration editor returns
- **WHEN** an external editor completes while the modal is still open
- **THEN** catalog refresh runs through the same bounded worker and retains modal identity.

### Requirement: Minimal passive inquiry frontier follows typed terminal phase

Minimal Pager SHALL retain a passive inquiry row in its live region while its phase is Received or Approved, regardless of the primary turn's animation state. It SHALL commit the row to native scrollback only after the row reaches Terminal.

#### Scenario: An inquiry progresses while the primary turn is idle
- **WHEN** a passive row is Received or Approved and the primary turn is idle
- **THEN** the row remains live and can accept a later terminal update before print-once commit.

#### Scenario: Inquiry reaches terminal
- **WHEN** the row's typed phase becomes Terminal
- **THEN** Minimal may commit it to native scrollback regardless of the primary turn state.

### Requirement: Switch Agent discovery does not block Pager input

Pager SHALL construct a prompt and open the `/agent` picker without filesystem-backed Agent discovery on the UI event path. The picker SHALL immediately offer built-in Agents and refresh from a bounded background scan of normal and plugin definitions. A failed or timed-out scan SHALL preserve the valid built-in choices; a late result SHALL apply only to the Agent view, session binding, and picker request that initiated it. Workflow Run children SHALL use their frozen Agent snapshot without a live scan.

#### Scenario: Filesystem-backed discovery stalls
- **WHEN** the user opens an Agent view or `/agent` picker while definition discovery remains blocked
- **THEN** prompt creation and picker input remain responsive, built-in choices remain selectable, and the request reports a finite failure after its deadline while the worker retains its permit until exit.

#### Scenario: Picker changes before discovery returns
- **WHEN** the user closes or replaces the picker, changes the Agent view's session binding, or opens a newer picker before an older scan finishes
- **THEN** the old result does not reopen or replace the current picker or catalog.

#### Scenario: Workflow child opens Agent picker
- **WHEN** a Workflow Run child has frozen Agent names
- **THEN** its picker lists exactly that snapshot and starts no live discovery.

### Requirement: Unified log disk writes do not block producers
Shell and Pager unified-log producers SHALL enqueue complete bounded records without waiting for a filesystem append, an inode lock, or trim. The process-local queue SHALL have a fixed record count and record byte bound. Overflow SHALL be observable as a coalesced diagnostic loss count; it SHALL NOT block a caller indefinitely or silently claim delivery. Snapshot and normal shutdown SHALL use a bounded flush wait and SHALL NOT claim to cancel an OS write already in progress.

#### Scenario: Filesystem append remains blocked
- **WHEN** the unified-log worker is held inside a slow append or inode lock
- **THEN** producer calls return after enqueue or bounded-queue overflow without waiting for that write.

#### Scenario: Queue saturates and later drains
- **WHEN** the worker falls behind enough to fill the queue and later resumes
- **THEN** dropped records are counted and a complete diagnostic record reports the coalesced loss before subsequent accepted records.

#### Scenario: Snapshot or shutdown meets a blocked worker
- **WHEN** a flush barrier cannot enter or cross the queue within its deadline
- **THEN** the caller stops waiting and does not claim all queued records were written.

### Requirement: Unified log appends enforce the shared file limit
Each Grow unified-log append SHALL check the live opened inode's length while holding the same exclusive advisory lock as in-place trimming. If the complete append would exceed 5 MiB, the writer SHALL retain only the most recent complete JSONL lines from the existing bounded trim window before appending. The writer SHALL refuse an append that still cannot fit or whose inode cannot be safely trimmed. A completed Grow append SHALL NOT leave a valid shared log larger than 5 MiB; this guarantee assumes other appenders honor the same advisory lock.

#### Scenario: Several writers cross the capacity threshold
- **WHEN** independent Grow writers append enough complete records to cross 5 MiB between maintenance ticks
- **THEN** each completed append leaves the shared inode within 5 MiB and the retained tail consists of complete lines followed by the new record.

#### Scenario: Existing tail cannot be trimmed safely
- **WHEN** the append would exceed 5 MiB and the bounded trim window has no complete line boundary, or on Unix the path no longer names the locked inode
- **THEN** the append is rejected without increasing that inode's length.

### Requirement: Kitty overlay conversion bounds created output
On Unix, the spawned `sips` converter SHALL inherit a 100,000,000-byte process file-size limit before it writes output. The Rust PNG fallback SHALL accept at most 100,000,000 encoded result bytes, including an exact-fit result. Converter failure at either limit SHALL produce no converted preview bytes and release owned temporary files. These limits SHALL NOT be described as a bound on source decode memory or direct terminal rendering.

#### Scenario: Converter writes beyond its artifact limit
- **WHEN** the `sips` child attempts to grow an output file past 100,000,000 bytes
- **THEN** its write fails or the child terminates without creating a larger file, and Grow rejects and cleans up the conversion.

#### Scenario: Rust encoder crosses its output limit
- **WHEN** the PNG encoder writes an exact-limit result or attempts one more byte
- **THEN** the exact-limit result is accepted, while overflow fails without extending the result buffer or returning a converted preview.

### Requirement: Pager clipboard metadata probes have a caller deadline
On macOS, Pager SHALL run native clipboard change-count and image-type snapshot probes on one bounded background worker. Pager callers SHALL wait at most 10 ms for AppKit initialization and native messaging; a missed deadline or unavailable worker SHALL return unknown metadata. Late results SHALL NOT be applied to a newer probe. The native worker SHALL retain at most one queued probe beyond the active call. Explicit image reads SHALL use the bounded subprocess transfer path and SHALL NOT read image data through AppKit.

#### Scenario: Native metadata call stalls
- **WHEN** AppKit initialization or an Objective-C clipboard message does not return promptly
- **THEN** the Pager caller receives unknown metadata within its deadline, remains interactive, and a later probe may retry without spawning another native worker.

#### Scenario: Paste arrives during a native metadata stall
- **WHEN** a native metadata probe holds the pasteboard lock during an explicit image paste
- **THEN** image acquisition proceeds through the independent deadline-bound AppleScript subprocess path without waiting for the metadata lock.

#### Scenario: A metadata reply arrives after the caller's deadline
- **WHEN** a timed-out native probe finishes after another probe has started
- **THEN** its reply is discarded and cannot replace the newer probe's metadata.

### Requirement: Local draft quarantine retention is bounded
Local draft quarantine SHALL retain at most 64 regular-file or symbolic-link entries and at most 16 MiB in total. When either limit is exceeded, it SHALL delete the oldest quarantined entries first; entries with equal or unavailable modification times SHALL be ordered by filename. Symlinks SHALL be measured by their own metadata and SHALL not be followed. Other non-regular entries SHALL not be counted.

#### Scenario: Quarantine exceeds the file count
- **WHEN** a local draft is quarantined while the quarantine directory contains more than 64 regular-file or symbolic-link entries
- **THEN** the oldest files are removed until at most 64 remain, with filename ordering deciding equal-time entries

#### Scenario: Quarantine exceeds the byte budget
- **WHEN** a local draft is quarantined and regular-file plus symbolic-link metadata bytes exceed 16 MiB
- **THEN** oldest entries are removed until retained bytes are at most 16 MiB, even if the remaining entry count is below 64

#### Scenario: Quarantine timestamps are unavailable or tied
- **WHEN** multiple quarantine entries have equal or unavailable modification times
- **THEN** reclamation order is resolved by filename and is independent of directory enumeration order

#### Scenario: Symlink and other special quarantine entries
- **WHEN** the quarantine directory contains a symlink or another special entry
- **THEN** reclamation counts a symlink using its own metadata without following it, ignores other special entries, and continues to apply both limits

### Requirement: Image viewer loading owns an aggregate memory reservation

Background image viewer loads SHALL acquire a process-wide reservation before copying encoded source bytes and before allocating conversion workspace or output. The reservation SHALL cover conservatively budgeted in-process conversion memory and the actual retained encoded buffers, and SHALL remain owned by a loaded viewer or undelivered result until that owner is dropped. Admission failure SHALL complete the current viewer as a failed preview without blocking the input thread or installing partially loaded bytes.

#### Scenario: Concurrent viewers approach the process allowance

- **WHEN** retained viewer buffers and in-flight conversions would exceed the aggregate allowance
- **THEN** the next load fails before its unreserved allocation, while existing viewers remain usable and input remains responsive.

#### Scenario: Viewer closes or a stale result is discarded

- **WHEN** a viewer closes/reopens or a delayed background result no longer matches its target owner
- **THEN** its reservation is released with the dropped viewer/result, and that result cannot replace the current viewer.

#### Scenario: Conversion fails after admission

- **WHEN** decoding or conversion fails after source admission
- **THEN** temporary reservation is released and the viewer settles through the existing failed-preview path.

### Requirement: Debug firehose is a bounded attributed stream

When enabled, the debug firehose SHALL write complete lines to one selected file with role, process ID and session ID attribution; payload line breaks SHALL be escaped inside their record. Its producer SHALL enqueue without filesystem I/O through one bounded process queue and one disk worker. Each complete line SHALL be at most 65,536 bytes and the queue SHALL hold no more than 64 records. Oversized lines SHALL end with a truncation marker; queue overflow SHALL be observable as a coalesced loss marker when writing resumes. A bounded flush SHALL wait for previously accepted records without claiming to cancel a blocked OS write or guarantee durable sync.

#### Scenario: Concurrent sessions

- **WHEN** several sessions and fallback events produce debug records
- **THEN** the one selected stream contains independently attributable complete lines without creating per-session writer workers or files.

#### Scenario: Producer outpaces disk

- **WHEN** disk writing stalls and the pending queue fills
- **THEN** producers return without waiting for disk and the next successful write reports the number of lost records.

### Requirement: Debug firehose retains a bounded complete-line tail

Each cooperating debug writer SHALL lock the opened inode exclusively, check its live size and keep the file at or below 32 MiB after a completed append. An overflowing append SHALL retain only complete recent lines from a bounded tail before adding the new complete record. The writer SHALL detect path replacement before append and reopen or drop rather than knowingly write to a detached descriptor. Age-based cleanup of legacy per-session files SHALL spare cooperating open writers.

#### Scenario: Shared path reaches its ceiling

- **WHEN** multiple Grow processes append enough complete records to exceed 32 MiB
- **THEN** each completed append preserves complete recent lines and the shared file does not exceed the ceiling.

#### Scenario: Selected path is replaced

- **WHEN** the debug path no longer names the worker's opened inode before its next append
- **THEN** the worker reopens the selected path or drops the record without continuing on the known detached inode.

### Requirement: Non-macOS clipboard image work has an in-process allowance
Non-macOS arboard image reads SHALL hold one process-wide permit through read and PNG encode, including after a caller timeout. Grow SHALL reject returned RGBA images over 16,000,000 pixels before PNG encoding, and SHALL refuse PNG output over 50,000,000 bytes. The limits SHALL NOT be described as a bound on platform RGBA allocation before `get_image` returns.

#### Scenario: Timed-out image reader remains active
- **WHEN** an arboard image worker has not returned by the caller deadline and a second image read starts
- **THEN** the second read does not start another in-process image worker while the first remains active.

#### Scenario: Oversized RGBA image or PNG output
- **WHEN** returned dimensions exceed the pixel allowance or PNG encoding would exceed the output allowance
- **THEN** Grow returns an error without retaining an over-limit encoded image.

### Requirement: Linux clipboard image helper capture is bounded
Linux CLI image fallback SHALL retain at most 50,000,001 stdout bytes, and SHALL reject an image whose output exceeds 50,000,000 bytes. A failed or over-limit helper SHALL not be reported as an empty clipboard image.

#### Scenario: Helper writes beyond the image allowance
- **WHEN** a Linux clipboard helper writes more than 50,000,000 image bytes
- **THEN** Grow reports an over-limit error and reaps or kills the helper within the existing deadline.

### Requirement: Subagent permission groups preserve their source epoch

Pager SHALL append or merge subagent permission events into a permission group only when the source permission epoch matches that group's epoch. Membership mutation SHALL be mediated by the scrollback state path that owns and compares those epochs; an event from an older or newer epoch SHALL NOT alter the group.

#### Scenario: Append within the active epoch
- **WHEN** a permission event is appended while its source epoch matches the active group
- **THEN** the group retains the event and invalidates its rendered projection.

#### Scenario: Append across an epoch boundary
- **WHEN** a permission event from a different epoch is offered to an existing group
- **THEN** the group remains unchanged and the state handles the event in the appropriate current group.

#### Scenario: Reconnect merge crosses a terminal boundary
- **WHEN** reconnect tail groups are merged and a source group's epoch differs from the destination group's epoch
- **THEN** their members remain separate and no event crosses the terminal boundary.

### Requirement: Subagent permission audit details are bounded and redacted

Live and durable subagent permission audit projections SHALL use the same bounded, redacted access summary and harness-owned decision reason. They SHALL NOT expose raw access detail or free-form classifier prose. The visible summary SHALL retain the tool identity and access kind where available; request arguments, paths, commands, URL credentials, path, query and fragment SHALL remain redacted. The projected access summary SHALL be at most 240 bytes, including its tool identity.

#### Scenario: Live permission decision contains sensitive request data
- **WHEN** a subagent permission decision contains a command, path, MCP arguments, URL credentials, or classifier prose with sensitive content
- **THEN** the live Pager detail displays only bounded redacted audit fields and contains none of the raw request or classifier prose.

#### Scenario: Permission decision is replayed
- **WHEN** Pager reconstructs the same decision from durable session updates
- **THEN** it displays the same bounded redacted audit projection as the live notification.

### Requirement: Pager validates permission selection against the queued request

Pager SHALL accept a permission selection only when its option ID belongs to the exact front request in the active Agent's permission queue. It SHALL perform this check before removing the request, sending a response, changing session permission mode, or applying any other selection side effect. An invalid selection SHALL leave the request queued and its response channel open, with no permission-mode change.

#### Scenario: Selection ID was not offered for the front request
- **WHEN** a selection names an option ID absent from the front request, including the global Always Approve ID on a child request
- **THEN** Pager leaves the queue and session mode unchanged and sends no response.

#### Scenario: Selection ID belongs to the front request
- **WHEN** a selection names an option ID offered by the front request
- **THEN** Pager handles it using the existing response, queue-transition, and applicable mode-change behavior.

### Requirement: Pager permission routing requires an exact session owner

Pager SHALL admit an ACP permission request only when its session ID exactly matches a registered root session or registered child view. The startup fallback used to route ordinary notifications to an active root whose session ID has not yet been assigned SHALL NOT apply to permission requests. An unmatched request SHALL be cancelled without being queued or changing session permission mode.

#### Scenario: Stranger permission arrives during root startup
- **WHEN** a permission request carries an unmatched session ID while the active root has no assigned session ID
- **THEN** Pager cancels the request and does not queue it on the active root.

#### Scenario: Benign update arrives during root startup
- **WHEN** an ordinary session update arrives before the active root's session ID is assigned
- **THEN** Pager retains the existing startup routing behavior for that update.

#### Scenario: Exact root or registered child requests permission
- **WHEN** a permission request carries an exact registered root or child session ID
- **THEN** Pager routes it to the owning root interaction queue using the existing behavior.

### Requirement: Terminal image escape buffers have a strict output budget
Grow SHALL construct each iTerm2 or Kitty image-upload escape buffer, and each buffered inline-media draw or clear accumulator, with no more than 100,000,000 serialized bytes, including protocol headers and chunk framing. Accumulators SHALL share that budget across placements, obsolete-ID clears, and recursively drained agent/subagent clear state; dashboard stale clears SHALL also reserve space for the popup inline-media output they precede. Grow SHALL reject an image upload whose complete escape output does not fit and SHALL NOT return a partial upload sequence. Grow SHALL encode into the bounded output buffer without first materializing a full-size base64 string. This budget covers Grow-owned escape output only and SHALL NOT be described as limiting terminal-process decode or cache memory. Fixed-size modal/subsession clears written directly to stderr and unrelated notification escapes are separate output paths, not members of the inline-media buffer budget.

#### Scenario: Kitty upload fits the output budget
- **WHEN** the complete Kitty upload escape sequence is at most 100,000,000 bytes
- **THEN** Grow returns the complete sequence with its existing chunk framing and transmission semantics

#### Scenario: Image upload exceeds the output budget
- **WHEN** Kitty or iTerm2 framing plus encoded image data would exceed 100,000,000 bytes
- **THEN** Grow returns no image upload sequence and does not retain or expose a partial sequence

#### Scenario: Buffered inline-media accumulator exceeds the output budget
- **WHEN** appending another complete placement or clear escape would make its buffered inline-media accumulator exceed 100,000,000 bytes
- **THEN** Grow omits that escape atomically and keeps the accumulator within the limit

#### Scenario: An old Kitty image clear does not fit
- **WHEN** the current frame has no room for an old Kitty image's complete clear escape
- **THEN** Grow leaves the clear out of the frame and retains its image ID for a later clear attempt

#### Scenario: Recursive or dashboard clear aggregation exceeds the output budget
- **WHEN** own, child, or another dashboard agent's Kitty clear does not fit the shared clear budget
- **THEN** Grow retains that image ID for a later clear attempt and emits no bytes beyond the budget

#### Scenario: Terminal decodes an accepted image
- **WHEN** a terminal receives an image escape sequence returned by Grow
- **THEN** Grow's output limit makes no claim about memory allocated by the terminal to decode or cache that image

### Requirement: Inline-media loading has bounded worker and payload admission
Pager SHALL perform inline-media filesystem reads and image preparation off the UI thread. It SHALL admit at most two such workers process-wide, at most two pending paths per AgentView, read at most 16 MiB of source bytes per path, and retain at most 16 MiB of prepared bytes per completion. Preparation may use its existing transient 100 MB conversion-output allowance per worker; output over 16 MiB SHALL be discarded before mailbox admission. A request rejected only because of worker or pending saturation SHALL remain eligible for a later render retry. Genuine read or preparation failures SHALL retain the existing bounded rename-race retry and failed-path behavior. A result from a mailbox detached at a session boundary SHALL NOT enter the replacement session's cache.

#### Scenario: Worker capacity is saturated
- **WHEN** an inline-media path is requested while both worker permits are occupied
- **THEN** Pager does not block the UI, does not mark the path pending or failed, and may request it again on a later render

#### Scenario: Per-view pending capacity is saturated
- **WHEN** an AgentView already has two inline-media paths pending
- **THEN** another path is not admitted or marked failed and remains eligible for a later render retry

#### Scenario: Source or prepared image exceeds the byte limit
- **WHEN** source bytes or prepared image bytes exceed 16 MiB
- **THEN** Pager does not retain those bytes in a completion mailbox or CPU cache

#### Scenario: Session changes during inline-media loading
- **WHEN** a worker completes after its AgentView has reset the inline-media loader
- **THEN** the completion remains in the detached old mailbox and cannot populate the replacement session cache

### Requirement: Cold replay paint work is bounded by a visible progress cadence
While a visible session is replaying historical notifications, Pager SHALL apply a minimum 100 ms interval between automatically requested paints from ACP replay, animation deadlines, and periodic UI maintenance, unless the configured interval is slower. Explicit user input and other direct UI actions SHALL retain their existing immediate redraw behavior. When the session-loaded boundary arrives, Pager SHALL return to the normal configured cadence and show the final replay state and prompt without waiting for another replay interval. The paint policy SHALL NOT drop notifications, reorder replay, or admit a prompt before the existing load barrier.

#### Scenario: Long history streams faster than painting
- **WHEN** a visible cold session receives many historical notifications while `loading_replay` is true
- **THEN** automatic ACP, animation, and periodic maintenance paints use at least a 100 ms interval while notifications continue to be processed in order.

#### Scenario: User types during replay
- **WHEN** terminal input arrives during a replay interval
- **THEN** its existing immediate handling and redraw are not delayed by the automatic paint cadence, and the draft remains until the load barrier permits submission.

#### Scenario: Replay completes
- **WHEN** the session-loaded boundary ends `loading_replay`
- **THEN** the final history and prompt use the normal configured paint cadence rather than waiting for a pending replay-only interval.

### Requirement: macOS clipboard transfer files have a per-file write budget
macOS clipboard AppleScript subprocesses SHALL inherit a regular-file size limit no greater than 50,000,000 bytes. Format fallback SHALL catch image coercion failures only; once coercion succeeds, transfer-file open/write/close failures SHALL propagate as an image-read error and the owned private temporary directory SHALL be cleaned. This bounds each file, not aggregate temporary storage, helper/AppKit memory, or later image decoding.

#### Scenario: Transfer file exceeds the write budget
- **WHEN** an AppleScript subprocess attempts to write more than 50,000,000 bytes to one transfer file
- **THEN** the write cannot extend that file beyond the limit, the clipboard operation returns an error, and its private temporary directory is removed.

#### Scenario: Transfer file fits the write budget
- **WHEN** an AppleScript subprocess writes at most 50,000,000 bytes to one transfer file
- **THEN** the child file-size limit does not prevent the write and existing clipboard result handling remains available.

### Requirement: Local draft filesystem latency does not block Pager interaction

Pager SHALL perform local-draft cwd resolution, load, rekey, write, remove, and quarantine I/O outside its input and paint event loop. A stalled draft filesystem operation SHALL NOT delay key handling or drawing. Ordered worker commands SHALL preserve prompt-RPC invalidation before a later local draft write. A recovered draft SHALL apply only to its still-current agent/session/cwd binding and only when live unsent input has not superseded the load. Normal quit SHALL wait for a checkpoint of the latest eligible draft and pending invalidations; a filesystem failure SHALL retain retry intent without a busy loop.

#### Scenario: Slow draft store during editing

- **WHEN** a draft read or write is held by a controlled slow filesystem operation while the user types
- **THEN** Pager continues to handle and draw the new input, and the eventual disk completion cannot replace that newer input.

#### Scenario: Prompt ownership changes while disk work is pending

- **WHEN** a prompt RPC transfers draft ownership while an older write or load is in flight
- **THEN** invalidation is ordered after old work, a stale recovery cannot restore submitted text, and a subsequent new draft can persist without being deleted by the old invalidation.

#### Scenario: Quit with pending draft work

- **WHEN** Pager quits while the latest eligible draft or invalidation is still pending
- **THEN** its checkpoint waits for the ordered worker outcome and does not report a successful normal exit before the attempt completes.

### Requirement: Independent notifications bypass retractable candidate buffering

The leader SHALL retain only attempt-owned provisional notifications in its mixed-client retractable candidate buffer. Untagged independent ACP and Grow notifications SHALL continue through the normal live route to observers that cannot retract provisional output, even while a candidate is active. A discarded candidate SHALL remain hidden from those observers; an accepted candidate SHALL be delivered after admission. Durable replay SHALL retain its canonical ordering.

#### Scenario: Independent update during an active candidate

- **WHEN** an untagged independent ACP or Grow notification arrives between provisional candidate fragments
- **THEN** a non-retracting observer receives that independent notification live, while the candidate remains withheld until its terminal boundary.

#### Scenario: Candidate is discarded after independent update

- **WHEN** an independent notification has been delivered during a candidate that is later discarded
- **THEN** the independent notification remains visible exactly once and no candidate fragment is delivered to a non-retracting observer.

### Requirement: Leader candidate retention has a finite spool budget

The leader SHALL retain retractable candidate payloads for non-retracting observers in one transient spool per session. The spool SHALL keep no more than 8 MiB in its in-process buffer before spilling to an unlinked temporary file, and SHALL accept no more than 512 MiB of length-prefixed serialized payloads or 1,000,000 records for one candidate. An accepted candidate SHALL be forwarded in order without materializing the full spool again. A discarded or superseded candidate SHALL release the spool without delivery. A spool budget or I/O failure SHALL discard that candidate's transient spool while preserving subscriptions and independent notification delivery. If the candidate is Accepted, the leader SHALL request an in-place full canonical reload for non-retracting observers after any in-flight load; Grow Pager SHALL perform that reload for its attached root, including when the failed candidate belongs to a child session. If Discarded, the leader SHALL NOT request resync. No unaccepted candidate payload SHALL reach those observers.

#### Scenario: Long candidate crosses memory threshold

- **WHEN** a retractable candidate's serialized records exceed 8 MiB but remain below the disk and record limits
- **THEN** the leader spills them to a temporary file and delivers them in original order only after Accepted.

#### Scenario: Candidate is discarded after spill

- **WHEN** an attempt is discarded after its provisional records have spilled
- **THEN** the spool is released and a non-retracting observer receives none of those records.

#### Scenario: Candidate exceeds spool budget

- **WHEN** a candidate record exceeds the byte or count ceiling before Accepted
- **THEN** the leader releases its spool, keeps its clients and session ownership, and withholds all provisional records from non-retracting observers.

#### Scenario: Failed candidate is accepted or discarded

- **WHEN** a failed candidate reaches Accepted, including after a spool read fails mid-flush
- **THEN** the leader signals each affected non-retracting observer after any in-flight load; Grow Pager reloads canonical history in place, replacing any partial presentation.
- **WHEN** a failed candidate reaches Discarded
- **THEN** those observers receive no candidate record and require no resync.

#### Scenario: Observer attaches after failure

- **WHEN** an observer loads the session after the failed candidate has reached a terminal boundary
- **THEN** its normal durable load supplies accepted history without transient backfill.
