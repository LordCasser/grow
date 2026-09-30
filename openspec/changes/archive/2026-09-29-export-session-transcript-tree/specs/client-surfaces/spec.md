## ADDED Requirements

### Requirement: CLI export writes a session transcript directory tree

`grow export <session-id> [output-dir]` SHALL 导出指定 session 与其已持久直接委派后代的 Markdown 目录树。默认输出根 SHALL 是调用 cwd 下以完整 canonical session ID 命名的目录；显式 output-dir SHALL 直接作为输出根，展开 `~` 后按调用 cwd 解析相对路径。每个节点 SHALL 写 `transcript.md`，直接 child SHALL 位于该节点的 `subagents/<child-session-id>/`，递归保持实际委派关系。CLI SHALL 不再提供 transcript stdout 或 `--clipboard` / `-c` 模式。

#### Scenario: Default export includes nested delegates
- **WHEN** 指定 session 有 child 和 grandchild，各自有独立对话
- **THEN** 当前目录产生 `<session-id>/transcript.md`、`subagents/<child-id>/transcript.md` 和其下的 `subagents/<grandchild-id>/transcript.md`，每份正文来自各自 session。

#### Scenario: Explicit destination differs from session cwd
- **WHEN** 指定相对或绝对 output-dir，session cwd 与命令 cwd 不同
- **THEN** 只按 output-dir 规则选择根目录，不使用 session cwd，也不在显式根下额外追加 session ID。

#### Scenario: Clipboard flag is supplied
- **WHEN** 调用 CLI export 带 `--clipboard` 或 `-c`
- **THEN** 参数解析拒绝该选项，不进行导出或剪贴板操作。

#### Scenario: Empty valid session
- **WHEN** 一个有效节点没有可见正文
- **THEN** 它仍具有带 session 身份和无已记录正文说明的 transcript，不从树中静默消失。

证据与实施入口：`crates/codegen/pager/src/export_cmd.rs::ExportArgs/run`、`crates/codegen/pager/src/app/cli.rs::Command::Export`。

### Requirement: Transcript hierarchy follows verified direct delegation

导出 SHALL 从 durable spawn/seed 事实验证 child identity、lifecycle owner 和直接委派者，并以直接委派者组织目录。Summary parent、展示缓存或目录扫描 SHALL NOT 单独作为委派 authority。每个包含节点 SHALL 唯一且具有可验证的父链；现有 terminal result reference SHALL 经过对应链接校验。请求某个 child 为根时 SHALL 只导出该 child 的委派子树；fork 祖先、resume 来源、兄弟 session 和 Sideband SHALL NOT 被当作后代。

#### Scenario: Nested lifecycle is stored in the root ledger
- **WHEN** child 和 grandchild 的 spawn 都由 root Timeline 持有，但 grandchild 的直接安全父身份指向 child
- **THEN** grandchild 文档位于 child 目录下，不被扁平放在 root 下，spawn/seed 的 lifecycle owner 链仍按真实来源校验。

#### Scenario: A subagent is the requested export root
- **WHEN** 请求导出一个 subagent，其后代的 lifecycle 记录位于更高层 owner
- **THEN** 读取必要 owner 索引后只导出请求根及其直接委派后代，不输出 owner、兄弟或源会话的 transcript。

#### Scenario: Fork or resume provenance exists
- **WHEN** session 带有 fork parent 或 resumed_from
- **THEN** 它们保持来源语义，不被转化为新的目录父子边或导致祖先历史重复遍历。

#### Scenario: Required child data is missing or conflicting
- **WHEN** 一个选中 child 缺少必要 ledger、seed/result link 不匹配、出现循环或多父身份
- **THEN** 整次导出明确失败并定位相关 session，不返回遗漏 child 的成功结果，也不执行修复或恢复。

#### Scenario: A parent transcript was rewound
- **WHEN** 父 session 当前对话分支回退，但仍有已持久的委派 spawn/seed
- **THEN** 委派目录清单保留该事实，每个节点的正文分别遵循自身当前分支，不复活被 rewind 排除的正文。

证据与实施入口：`chat-state/src/timeline.rs::SubagentSpawnEvent/Timeline::validate_subagent_seed_link/validate_subagent_result_link`、`shell/src/agent/subagent/mod.rs::SubagentCtx`、`shell/src/session/trajectory.rs::refresh_tree_from_directory`；路径均相对 `crates/codegen/`。

### Requirement: Full transcript export preserves recoverable conversation presentation

完整会话树导出 SHALL 使用与会话界面一致的用户可见内容选择、正文与状态语义，覆盖可恢复的 ACP 和 Grow 会话展示，包括 thinking、工具结果和子 agent 生命周期。它 SHALL 保留现有隐藏用户回显、显示文本、response admission、rewind、去重和 Hook observational 规则，不把模型 Surface、原始 provider 请求或 trajectory 调试行作为用户消息。每份文档 SHALL 包含身份及直接子文档的相对链接；child 正文 SHALL 保存在 child 文档中。

#### Scenario: Display text differs from model input
- **WHEN** 用户消息携带 display text、skill 包装或 hideFromScrollback 标志
- **THEN** transcript 采用相同展示选择，不泄漏包装后的内部输入或伪造用户消息。

#### Scenario: Conversation contains reasoning and tool details
- **WHEN** 会话包含可见 thinking、工具参数展示、结果、错误或已有截断标记
- **THEN** Markdown 保留相应正文和状态，不再仅输出工具一行摘要或无条件省略 thinking。

#### Scenario: Conversation contains Grow presentation
- **WHEN** 已保存或可从事实重建子 agent、通信、Hook、压缩、后台任务、Goal 或 Workflow 会话展示
- **THEN** 导出保留相应用户展示，父级生命周期不代替完整 child transcript，历史 Hook 不重新执行。

#### Scenario: Response cache is incomplete or stale
- **WHEN** canonical response 需要从 Timeline 补投影，或存在 discarded/quarantined/rewound candidate
- **THEN** 与现有 reconciliation 一致地重建去重历史，不复活排除内容、不猜测无法证明的响应位置。

#### Scenario: Optional display material is unavailable
- **WHEN** 已记录的附件引用或截断结果缺少可选展示材料
- **THEN** 保留可得引用及明确 unavailable/truncated 说明，不把当前工作区文件或重新执行结果充当历史正文；必要 authority 损坏仍失败。

#### Scenario: Existing interactive export is used
- **WHEN** 用户调用 TUI `/export` 或既有 `/transcript`
- **THEN** 继续输出当前活动 root/child 视图的单份完整 transcript，保留紧凑展示、路径和剪贴板语义，不自动升级成目录树或仅导出视窗可见的一页。

证据与实施入口：`pager/src/acp/tracker.rs`、`pager/src/scrollback/export.rs`、`pager/src/scrollback/block.rs`、`pager/src/app/acp_handler/`、`pager/src/app/root/dispatch/transcript.rs`；路径均相对 `crates/codegen/`。

### Requirement: Transcript tree export is bounded and observational

会话树导出 SHALL 固定每个必要来源的已提交读取边界，复用身份绑定的只读存储能力，不取得 writer lease、不修复 Summary/Timeline、不开启执行 session。读取 SHALL 有全树节点、深度、事件及实际源字节预算，输出 SHALL 有总字节预算；超限、不可验证来源和必要材料缺失 SHALL 明确失败。跨活跃 ledger 的读取 SHALL NOT 被宣称为同一时刻的全局原子快照。

#### Scenario: Export while a writer exists
- **WHEN** 源 session 有活跃 writer 且选定来源可以完整校验
- **THEN** 导出只消费已固定边界内的记录，不争抢 writer、不追随后续追加，源文件内容不因导出改变。

#### Scenario: Captured references are not yet complete
- **WHEN** 固定读取边界下某个必要跨 ledger 前件不可用
- **THEN** 导出报告不完整来源并允许之后重试，不把它解释为无子 agent 或空对话。

#### Scenario: Source or output exceeds its budget
- **WHEN** 节点、深度、记录、实际读取字节或生成 Markdown 达到限制
- **THEN** 操作停止并报告超限，不静默截掉剩余 agent 后返回成功。

证据与实施入口：`shell/src/session/storage/jsonl/mod.rs::open_session_by_id_shared_read`、`shell/src/session/storage/mod.rs::reconcile_raw_replay_lines` 与本 change design 的预算；路径均相对 `crates/codegen/`。

### Requirement: Transcript directory publication never overwrites an existing target

CLI 导出 SHALL 在私有同级临时目录中完成所有 transcript 的生成与写入，再以不覆盖的目录发布提交到最终路径。任何已有目标 SHALL 被拒绝，包括普通文件、空目录、非空目录及符号链接。提交前失败 SHALL 保持已有目标不变并清理本次临时输出；成功反馈 SHALL 晚于完整目录提交。

#### Scenario: Export succeeds
- **WHEN** 所有必要节点和文件均准备成功且最终路径不存在
- **THEN** 一次发布完整目录树，再报告路径与 agent 数量。

#### Scenario: Destination already exists or appears during publication
- **WHEN** 最终路径预先存在，或在准备和发布之间被其他操作创建
- **THEN** 导出失败，已有路径不被覆盖或合并，哪怕它是空目录。

#### Scenario: Child rendering or writing fails
- **WHEN** 某个后代已经写入临时目录后发生读取、投影或写入失败
- **THEN** 清理本次临时目录，不留下表示完整导出的最终路径，不报告成功。

证据与实施入口：`pager/src/export_cmd.rs`；参考 `shell/src/session/storage/mod.rs::ContainedDirectory::publish_child_no_replace` 和 `pager/src/local_drafts.rs::rename_no_replace` 的原语，不复用草稿业务；路径均相对 `crates/codegen/`。

### Requirement: Trajectory downloads the same transcript tree

trajectory SHALL 为页面绑定的 session 提供完整对话导出下载，内容是与 CLI 相同布局和正文规则的 `<session-id>.tar.gz`，归档内有 `<session-id>/` 顶层目录。导出 SHALL 忽略调试器的行过滤和当前分页范围，沿用 loopback、随机 token 与 Host 校验，不接受任意服务器文件输出路径。下载 SHALL 有界并持有临时产物的明确释放责任。

#### Scenario: Download from a filtered trajectory page
- **WHEN** 页面带 search/layer/visibility 等筛选，用户选择导出对话
- **THEN** 下载绑定 session 的完整会话树，而不是当前已加载或筛选出的调试行。

#### Scenario: Compare CLI and browser output
- **WHEN** 两个入口消费相同的固定来源快照
- **THEN** 解包后的相对路径和 Markdown 内容与 CLI 输出一致，压缩包元数据不影响比较。

#### Scenario: Concurrent request or disconnected download
- **WHEN** 一个导出仍在执行，或下载客户端断开
- **THEN** server 不无限堆积任务；并发请求得到明确 busy 反馈，当前产物在 owner 结束后清理，不写入源 session。

证据与实施入口：`shell/src/session/trajectory.rs::serve/trajectory_router`、`shell/src/session/trajectory.html`、`pager/src/trajectory_cmd.rs`；路径均相对 `crates/codegen/`。回调由 Pager 组合，Shell 不反向依赖 Pager。

## MODIFIED Requirements

### Requirement: Transcript file export commits completed content atomically

TUI 显式 transcript 文件导出 SHALL 先完整写入同目录临时文件并同步，再原子替换普通目标；提交前失败 SHALL 保留旧内容并清理临时文件。CLI 会话树导出的目录提交 SHALL 遵循 `Transcript directory publication never overwrites an existing target`，不得把单文件原子性当作整棵树发布保证。

#### Scenario: Partial temporary write fails
- **WHEN** 临时文件已写入部分内容后发生错误
- **THEN** 导出失败，原目标内容保持不变且临时文件清理。

#### Scenario: Existing symbolic link
- **WHEN** TUI 显式文件目标为指向现存普通文件的符号链接
- **THEN** 提交替换链接目标，保留符号链接；悬空链接报错。

#### Scenario: Permissions and special targets
- **WHEN** TUI 显式文件目标为已有普通文件、新文件或非普通文件
- **THEN** 已有权限保留且只读目标拒绝；Unix 新文件默认私有权限；非普通目标拒绝。

## REMOVED Requirements

### Requirement: CLI clipboard export reports actual delivery
**Reason**：用户明确选择 CLI export 统一输出分层目录，移除 `--clipboard` / `-c`，不再有 CLI clipboard export 投递路径。
**Migration**：使用 `grow export <session-id> [output-dir]` 获得完整树；需要当前交互页面剪贴板导出时继续使用 TUI `/export`，其现有投递反馈保持。
