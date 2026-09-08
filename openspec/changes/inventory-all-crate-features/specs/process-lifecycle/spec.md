## ADDED Requirements

### Requirement: Terminal detach command hooks
detach_command 和 detach_std_command SHALL 在 Unix pre_exec 调用 setsid，EPERM 时回退 setpgid(0,0)，其他失败返回 errno；Windows 设置 CREATE_NO_WINDOW。

#### Scenario: 回退边界
- **WHEN** setsid 因已是进程组 leader 失败但 setpgid 成功
- **THEN** 只有进程组隔离，仍可能访问 controlling TTY；辅助函数不自动重定向 stdin/stdout/stderr。

#### Scenario: Windows flags
- **WHEN** 依次调用多个 creation_flags helper
- **THEN** 各 helper 设置自己的 flag 值，不保证自动合并；新进程组 helper 单独设置 CREATE_NEW_PROCESS_GROUP。

证据：`crates/codegen/tty-utils/src/lib.rs` — `detach_from_tty`；`crates/codegen/tty-utils/src/lib.rs` — `detach_command`；`crates/codegen/tty-utils/src/lib.rs` — `new_process_group`。

### Requirement: Linux parent death binding
kill_on_parent_death_std SHALL 仅在 Linux 注册 pre_exec，设置 PR_SET_PDEATHSIG(SIGTERM)，并核对当前 ppid 与 arm 时捕获的父 PID，不一致立即 _exit(0)；其他平台无操作。

#### Scenario: 线程与权限边界
- **WHEN** debug 下 arm 与 spawn 来自不同线程
- **THEN** 返回 EINVAL；实际绑定的是 spawning thread 生命周期，release 不强制线程检查，setuid/setcap exec 可清除设置。

#### Scenario: child 主动绑定
- **WHEN** 调用 kill_current_process_on_parent_death
- **THEN** Linux 设置相同信号并返回 prctl 错误，不检查 arm 前父进程已死亡的竞态，需 caller EOF 处理；其他平台 Ok 无操作。

证据：`crates/codegen/tty-utils/src/lib.rs` — `kill_on_parent_death_std`；`crates/codegen/tty-utils/src/lib.rs` — `bind_to_parent_death`；`crates/codegen/tty-utils/src/lib.rs` — `kill_current_process_on_parent_death`。

### Requirement: Validated process group teardown
Unix ProcessGroupId SHALL 拒绝 0、1、大于 i32::MAX 及调用者自身 pgid；ProcessGroup attach/attach_std/attach_pid 保存校验后的 group leader，terminate/kill 分别 killpg SIGTERM/SIGKILL。

#### Scenario: 未登记与重用
- **WHEN** 未 attach 的 group 调用 kill 或已有 leader 被重用
- **THEN** 未 attach 为 Ok 无操作；attach 不核实目标实际 pgid 或持有身份句柄，caller 必须保证 group leader 条件并在 reap 时释放 handle。

#### Scenario: hangup
- **WHEN** Unix terminal group 请求 hangup
- **THEN** 先 SIGHUP 再 SIGCONT，便于停止的 shell 转发信号；Unix Drop 不杀进程，信号 errno 向直接 caller 传播，跨组 job 不由 killpg 直接覆盖。

证据：`crates/codegen/tty-utils/src/lib.rs` — `ProcessGroupId`；`crates/codegen/tty-utils/src/lib.rs` — `ProcessGroup`；`crates/codegen/tty-utils/src/lib.rs` — `hangup`。

### Requirement: Windows job enrollment and resume
Windows ProcessGroup SHALL 新建设置 KILL_ON_JOB_CLOSE 的 Job Object，以 PROCESS_SET_QUOTA|PROCESS_TERMINATE 打开目标后分配入 job，关闭临时 handle；terminate/kill 都以退出码 1 终止 job，Drop 关闭 job。

#### Scenario: 凭据 helper 启动
- **WHEN** 调用 suspend_until_process_group_enrolled 后 spawn/attach/resume
- **THEN** 启动 flags 包含 NEW_PROCESS_GROUP/NO_WINDOW/SUSPENDED；resume_enrolled 枚举目标 PID 首个线程并 ResumeThread，找不到或 API 失败返回错误，调用者须保证先 attach 后 resume，本方法不独立校验所属 job。

#### Scenario: 平台差异
- **WHEN** 非 Windows 调用 suspend/resume
- **THEN** 无操作；Windows hangup 无操作且 wants_hangup 为 false。

证据：`crates/codegen/tty-utils/src/lib.rs` — `suspend_until_process_group_enrolled`；`crates/codegen/tty-utils/src/lib.rs` — `resume_enrolled`；`crates/codegen/tty-utils/src/lib.rs` — `attach_pid`。

### Requirement: Weak owned process scope
ProcessScope SHALL clone 共享 scope，注册 Weak<ProcessGroup> 而由 caller 持有强 Arc；prepare 设置新 group，enroll attach 后注册，spawn 组合 prepare/spawn/enroll。

#### Scenario: 所有权结束
- **WHEN** caller drop 最后一个 group Arc
- **THEN** live_count 不再计入，kill_all 跳过该 Weak；该安全边界依赖 caller 在正确 reap 时释放，live_count 不探测操作系统进程。

#### Scenario: 关闭与注册竞态
- **WHEN** kill_all 在 groups 锁内取走列表并锁定 closed 后，另一个 register 到达
- **THEN** 普通 group 当场 best effort kill 并返回 false；已要求 hangup 的 bare register 不杀，须通过 enroll_terminal_pid 或 caller 自行负责。

#### Scenario: spawn 错误
- **WHEN** 子进程 spawn 成功但 group new/attach 失败
- **THEN** 错误上抛；本函数没有显式 child wait 或通用回滚，不能推导所有 enrollment 错误都已回收。

证据：`crates/codegen/tty-utils/src/process_scope.rs` — `ProcessScope`；`crates/codegen/tty-utils/src/process_scope.rs` — `register`；`crates/codegen/tty-utils/src/process_scope.rs` — `spawn`。

### Requirement: Scope hangup grace and cleanup
kill_all 和最后一个 ScopeInner Drop SHALL 对仍可升级的 Weak 执行清理：先 hangup 标记的 terminal group，有任何标记则共享等待 200ms，再重新升级 Weak 并 kill；信号错误 best effort 忽略。

#### Scenario: 等待期间 owner 释放
- **WHEN** hangup 后 grace 内最后 Arc 被 drop
- **THEN** 第二次升级失败则跳过，避免主动延长 group handle 生命周期；本层不 wait 子进程，owner 负责 reap。

#### Scenario: terminal 晚到与全局 scope
- **WHEN** enroll_terminal_pid 在 closed scope 或使用 global_process_scope
- **THEN** 晚到 terminal 走同样 hangup/grace/kill 并返回错误；global scope 是 OnceLock 单例，关闭后不重开，is_closed 只反映关闭 latch。

证据：`crates/codegen/tty-utils/src/process_scope.rs` — `reap_groups`；`crates/codegen/tty-utils/src/process_scope.rs` — `enroll_terminal_pid`；`crates/codegen/tty-utils/src/process_scope.rs` — `global_process_scope`。

### Requirement: Noninteractive git command environment
pager_env SHALL 将 PAGER/GIT_PAGER/GH_PAGER/MANPAGER/SYSTEMD_PAGER 设为 Unix cat 或其他平台空串，AWS_PAGER/GPG_TTY 为空，Git editor 为 Unix true 或 Windows cmd exit，GIT_TERMINAL_PROMPT=0。

#### Scenario: 构造 git 命令
- **WHEN** 调用 git_command
- **THEN** 应用 detach、stdin null、pager 环境及 GIT_ASKPASS 空串/GIT_LFS_SKIP_SMUDGE=1/GIT_SSH_COMMAND BatchMode，并添加 --no-optional-locks；不在构造时执行命令。

#### Scenario: 指定 Git
- **WHEN** 设置 GIT_BIN_PATH
- **THEN** 相对路径以当前目录解析，文件名恰为 git 时设置 GIT_EXEC_PATH 为父目录，其他 wrapper 保留自身 exec path；缺失使用 git。

证据：`crates/codegen/tty-utils/src/lib.rs` — `pager_env`；`crates/codegen/tty-utils/src/lib.rs` — `GIT_AUTH_SUPPRESSION_ENVS`；`crates/codegen/tty-utils/src/lib.rs` — `git_command`。

### Requirement: Native stderr redirection and duplication
redirect_native_stderr SHALL 在 Unix best effort 复制 fd2 到进程期 OnceLock，flush Rust stderr 后将 fd2 指向 devnull；dup_tui_stderr 返回独立拥有的原 stderr 副本，未重定向时复制当前 fd2。

#### Scenario: 恢复
- **WHEN** restore_native_stderr 已有保存 fd
- **THEN** Unix dup2 恢复原 stderr；redirect/restore 不返回失败，需在启动线程前由 caller 安排。

#### Scenario: Windows
- **WHEN** 调用上述接口
- **THEN** redirect/restore 无操作，dup 从 GetStdHandle 通过 try_clone 获得独立 handle，不关闭原标准错误 handle；不保证控制台 code page 的 Unicode 显示。

证据：`crates/codegen/tty-utils/src/lib.rs` — `redirect_native_stderr`；`crates/codegen/tty-utils/src/lib.rs` — `dup_tui_stderr`；`crates/codegen/tty-utils/src/lib.rs` — `restore_native_stderr`。

### Requirement: Cached WSL detection
is_wsl SHALL 缓存进程生命周期判定，非 Linux 为 false；Linux 通过 WSL_DISTRO_NAME/WSL_INTEROP 键存在或 osrelease 的不区分大小写 microsoft/wsl 子串判定。

#### Scenario: 环境变化
- **WHEN** 第一次检测后修改环境
- **THEN** 缓存不刷新；纯输入辅助检查按完整环境键匹配，值为空也视为存在，读取不到 osrelease 且无键则 false。

证据：`crates/codegen/tty-utils/src/lib.rs` — `is_wsl`；`crates/codegen/tty-utils/src/lib.rs` — `is_wsl_from_inputs`。

### Requirement: Tokio worker budget
capped_worker_threads SHALL 读取 available_parallelism，失败回退 1，并将返回值上限设为 8；cap_worker_threads 为 NonZeroUsize 纯函数。

#### Scenario: 多核主机
- **WHEN** 可用并行为 360
- **THEN** 返回 8；输入 1..8 原样返回，本包只返回预算，不自动创建或修改运行时。

证据：`crates/codegen/tty-utils/src/runtime.rs` — `capped_worker_threads`；`crates/codegen/tty-utils/src/runtime.rs` — `cap_worker_threads`。


### Requirement: Shell active session registry replacement and collection

活跃会话登记 SHALL 保存session_id/pid/cwd/opened_at；register按session_id删除旧项后追加新项，不按PID区分同ID。unregister删除所有匹配ID。collect_crashed按PID存活划分，持久化保留活项后返回移除项，不读取cwd/session事件、不验证进程启动身份；PID复用可能保留旧登记。list_in无锁读取。

#### Scenario: Duplicate identity
- **WHEN** 同session_id以不同pid再次register
- **THEN** 新记录替换旧记录并移至列表末尾。

证据：`crates/codegen/shell/src/active_sessions.rs` — `pub fn register_in`；`crates/codegen/shell/src/active_sessions.rs` — `pub fn collect_crashed_in`；`crates/codegen/shell/src/active_sessions.rs` — `pub fn list_in`。

### Requirement: Shell active session file locking and corruption recovery

登记变更 SHALL 创建root并以active_sessions.lock独占锁保护read-modify-write，固定tmp文件写漂亮JSON后rename至active_sessions.json，不fsync文件/目录。try_unregister只对锁争用返回false，其余操作仍为同步文件I/O，不能视作整个调用非阻塞或async-signal-safe。空文件/缺失返回空列表，坏JSON告警后视作空列表，后续变更可覆盖坏文件；其他读取错误传播。rename失败尽力删tmp，写tmp失败不保证清理。

#### Scenario: Corrupt prior state
- **WHEN** 坏JSON之后成功register
- **THEN** 原坏内容不会恢复，保存由空列表加新项形成的状态。

证据：`crates/codegen/shell/src/active_sessions.rs` — `fn with_locked_state`；`crates/codegen/shell/src/active_sessions.rs` — `fn try_with_locked_state`；`crates/codegen/shell/src/active_sessions.rs` — `fn read_data_file`；`crates/codegen/shell/src/active_sessions.rs` — `fn write_data_file_atomic`。

### Requirement: Shell active session platform liveness approximation

is_pid_alive SHALL 在Unix拒绝0及超i32正数范围PID，kill(pid,0)成功或EPERM视为活；其他错误视为死。Windows OpenProcess查询权限成功即活并关闭handle，任何错误视为死；其他平台总视为活。该判定不检测应用身份、不等待退出、不证明进程能响应请求。

#### Scenario: Permission boundary
- **WHEN** Unix kill(pid,0)返回EPERM
- **THEN** 保留登记为活；Windows查询失败没有对应EPERM保留分支。

证据：`crates/codegen/shell/src/active_sessions.rs` — `fn is_pid_alive`。


### Requirement: Shell helper task abort on scope drop
AbortOnDrop SHALL 在Drop调用所持JoinHandle的abort；它只请求任务取消，不await终止，也不提供子进程回收保证。 生产工具分发持有此guard管理FuturesUnordered结果drainer；receiver关闭使drainer停止发送，作用域退出发出abort，不在guard内join。

#### Scenario: Scope exits
- **WHEN** 持有guard的异步作用域正常退出或被取消
- **THEN** 向所持任务发出abort。

源码证据：
- `crates/codegen/shell/src/util/mod.rs` — `pub struct AbortOnDrop`。
- `crates/codegen/shell/src/util/mod.rs` — `impl Drop for AbortOnDrop`。


### Requirement: Shell detached subprocess execution and timeout phases
子进程辅助器 SHALL 使用null stdin、piped stdout/stderr、kill_on_drop、detach_command和pager_env；spawn失败返回SpawnFailed。ProcessGroup enrollment失败仍继续且显式终止仅保障直接child。timeout只包住child.wait，等待错误或超时进入terminate_child：有group先terminate、最多等500ms再kill，随后直接child start_kill并最多等500ms；错误返回不提供Output。非零正常退出仍返回Ok(Output)。本函数不保证spawn加清理加排空合计等于传入timeout，也不保证取消时后代全部回收。 当前crates源码检索仅发现本模块测试调用run_detached_with_timeout，未发现生产调用；上述为辅助器实现契约，不代表认证运行路径。

#### Scenario: Nonzero normal exit
- **WHEN** 进程在wait时限内以非零状态退出
- **THEN** 返回含status/stdout/stderr的Ok，退出状态由caller处理。

源码证据：
- `crates/codegen/shell/src/util/subprocess.rs` — `pub(crate) async fn run_detached_with_timeout`。
- `crates/codegen/shell/src/util/subprocess.rs` — `async fn terminate_child`。
- `crates/codegen/shell/src/util/subprocess.rs` — `pub(crate) enum RunError`。


### Requirement: Shell subprocess bounded capture and command resolution
输出读取 SHALL 每流最多保存1MiB，超过后继续读取丢弃，read错误视结束且无截断或读错标记；reader监听receiver关闭。child退出后两流并发排空，共享新起算2秒预算且各次无chunk250ms即结束，正常退出路径不显式终止后代。shell_c在Windows用cmd /C，其他平台sh -c；git_bin读取GIT_BIN_PATH，相对路径按当前cwd拼接，cwd失败保留原串，未设置才用git。CommandLog::Redacted显示<redacted>，Shown显示原文，RunOptions无自动默认选择。 当前git_bin未发现调用；认证mint_provider_token在args=None时使用shell_c构造命令，但执行使用独立run_capped，不继承本辅助runner的捕获/超时契约。

#### Scenario: Output exceeds cap
- **WHEN** stderr超过1MiB且child继续输出
- **THEN** 继续排空避免阻塞，仅前1MiB进入返回值，无truncated字段。

源码证据：
- `crates/codegen/shell/src/util/subprocess.rs` — `fn spawn_pipe_reader`。
- `crates/codegen/shell/src/util/subprocess.rs` — `async fn drain_reader`。
- `crates/codegen/shell/src/util/subprocess.rs` — `pub(crate) fn shell_c`。
- `crates/codegen/shell/src/util/subprocess.rs` — `pub(crate) fn git_bin`。
### Requirement: Pager TUI signal shutdown and terminal ownership

pager TUI SHALL 在terminal初始化后记录screen mode与owned状态；Unix安装时忽略SIGTTIN/SIGTTOU并异步等待SIGINT/SIGTERM/SIGHUP，分别映射130/143/129。首个信号在terminal仍owned且已注册quit Notify时只请求graceful quit，否则立即强制退出；等待到第二个信号总是强制退出。强制路径仅在owned时发terminal teardown、忽略disable_raw_mode错误、清owned并best-effort注销当前session；随后杀全局detached process scope、恢复native stderr、flush debug log并process::exit。SIGPIPE保持Rust默认SIG_IGN。Windows Ctrl+C使用首个graceful第二个force，console close立即code1，logoff/shutdown立即code0。

#### Scenario: First owned signal
- **WHEN** TUI拥有terminal且quit Notify已注册时收到首个受管信号
- **THEN** 只notify事件循环执行graceful quit，不在signal task立即恢复并exit。

#### Scenario: Forced second signal
- **WHEN** graceful请求后收到第二个受管信号
- **THEN** 按第二个信号code执行terminal恢复、detached child回收、日志flush和进程退出。

源码证据：`crates/codegen/pager/src/app/signal_handler.rs` — `install / spawn_async_signal_task / request_graceful_or_exit / shutdown_with_terminal_restore / flush_diagnostics_and_exit`。


### Requirement: Pager screen mode one shot override and process replacement

screen mode relaunch SHALL 仅接受精确`minimal`或`fullscreen`，不trim或忽略大小写。GROW_SCREEN_MODE及GROW_SCREEN_MODE_CONTROLS在启动读取后只要存在就立即从环境删除；非法mode返回None，非Unicodecontrol handoff返回空，JSON非法打印warning并返回空。preference按minimal CLI、fullscreen CLI、config顺序，双方CLI同时true时minimal优先；最终mode按one-shot env、minimal preference、alt-screen wants fullscreen顺序，否则Inline。relaunch设置mode env，control handoff非空时序列化设置私有env，并在Unix以exec替换当前进程；Windows忽略parent console Ctrl事件、等待150ms让input reader停下，以继承stdio启动child、wait并以child code退出，无法取code时0；其他平台返回unsupported。失败提示同时包含mode env、显式mode flag及resume ID。

#### Scenario: Forced fullscreen
- **WHEN** one-shot env解析为Fullscreen，即使minimal preference为true且alt-screen策略拒绝
- **THEN** 最终仍选择Fullscreen，并在读取后删除env。

#### Scenario: Malformed control handoff
- **WHEN** 私有handoff env为非法JSON
- **THEN** 删除变量、打印忽略warning并以空handoff继续reopen session。

源码证据：`crates/codegen/pager/src/app/screen_mode_relaunch.rs` — `exec_screen_mode_relaunch / parse_screen_mode / take_screen_mode_env_override / take_screen_mode_control_handoffs / effective_minimal_preference / resolve_screen_mode`。
### Requirement: Pager dashboard and screen mode slash relaunch routes

DashboardCommand SHALL 以名称agents返回OpenDashboard，不要求session；mode_support为FullscreenOnly并以minimal is single-session说明切换理由，该类别在现有测试中允许Fullscreen与Inline、排除Minimal。dashboard feature flag不在visible或run中读取，而由registry的set_dashboard_visible外部开关及dispatch再次拒绝负责。ScreenModeSwitchCommand::minimal与fullscreen分别生成to_minimal=true/false，名称、usage和description随目标变化；两者session_scoped，前者在非Minimal面提供且AlreadyInMode为remedy，后者只在Minimal面提供。run要求session_id存在后返回RelaunchInScreenMode{minimal:target}，Action不携带session ID；run不验证当前mode，实际argv重建、handoff和process replacement由relaunch层负责。

#### Scenario: Dashboard feature disabled direct run
- **WHEN** 绕过外部registry visibility直接调用/agents run
- **THEN** 仍产生OpenDashboard，最终拒绝依赖dispatch。

#### Scenario: Switch to minimal
- **WHEN** 当前有active session且调用minimal命令
- **THEN** 产生minimal=true的relaunch Action，不在命令对象保存session身份。

证据：`crates/codegen/pager/src/slash/commands/dashboard.rs` — `DashboardCommand::mode_support / run / visible_everywhere_except_minimal`；`crates/codegen/pager/src/slash/commands/screen_mode_switch.rs` — `ScreenModeSwitchCommand::minimal / fullscreen / target_label / mode_support / run`。
### Requirement: Pager background task cumulative stdout interception

A ToolCallUpdate SHALL be intercepted as background output only when its tool-call string maps to a task id; otherwise false delegates it to the normal tracker. A mapped update is consumed and returns true even without raw output, a known task row or extractable content. Only raw_output type Bash is examined: output_for_prompt string has priority, otherwise integer elements from output array are cast to u8 and decoded lossily. Nonempty output replaces the task's cumulative stdout through set_stdout; shell truncated true latches the task flag, while empty output never clears prior stdout. Non-Bash and malformed fields remain consumed without mutation.

#### Scenario: Not background
- **WHEN** the tool-call id has no task mapping
- **THEN** false is returned for normal tracking.

#### Scenario: Prompt output
- **WHEN** mapped Bash raw output has output_for_prompt
- **THEN** that string replaces task stdout when nonempty.

#### Scenario: Byte output
- **WHEN** no prompt string exists but output is an array
- **THEN** integer elements are cast to bytes and decoded with UTF-8 loss replacement.

#### Scenario: Empty completion
- **WHEN** the extracted cumulative buffer is empty
- **THEN** existing stdout is retained.

#### Scenario: Consumed malformed
- **WHEN** the mapped update lacks usable Bash content
- **THEN** true is still returned without task mutation.

证据：`crates/codegen/pager/src/app/acp_handler/background.rs` — `route_bg_task_stdout`。

### Requirement: Pager background task start demotion and monitor classification

A TaskBackgrounded session update SHALL route to the concrete root or child session, mark replay restores, drain the deferred-tool entry, and classify monitors from structured monitor_description or a `[monitor] ` command prefix. Description priority is nonblank monitor description, stripped prefix, notification description, deferred description, then an existing Execute description during demotion. New running BgTaskState uses current SystemTime and empty output. When a pending Execute entry exists it is converted in place to collapsed unpinned BgTask, carries prior output through set_stdout, stops running and leaves the tracker; if that entry vanished, or no demotion exists, a fresh running block is pushed. Task state, tool correlation and scrollback id are inserted, and only parent active status controls the return value.

#### Scenario: Fresh background
- **WHEN** no pending Execute entry exists
- **THEN** a running BgTask block and central state are created.

#### Scenario: Demotion
- **WHEN** the tracker points to an existing Execute block
- **THEN** output and fallback description migrate and the same entry becomes a collapsed BgTask.

#### Scenario: Vanished entry
- **WHEN** the tracker id no longer exists
- **THEN** pending tracking is removed and a fresh BgTask block is pushed.

#### Scenario: Monitor
- **WHEN** structured description or legacy prefix exists
- **THEN** the task is marked monitor and the prefix can supply a bare label.

#### Scenario: Replay
- **WHEN** notification meta marks history replay
- **THEN** restored_from_replay is set for later pane behavior.

证据：`crates/codegen/pager/src/app/acp_handler/background.rs` — `handle_task_backgrounded`。

### Requirement: Pager monitor event background stdout append

A MonitorEvent SHALL parse the SessionNotification, route to the addressed root or direct concrete child session, and append event_text through the matched task's bounded append_stdout path. Unknown task ids perform no mutation but still return the owning parent's active status. Malformed, wrong-variant, unmatched or missing child notifications return false. The payload description is ignored and this handler does not create a missing task or scrollback block.

#### Scenario: Known task
- **WHEN** routing succeeds and task_id exists
- **THEN** event_text is appended with stdout trimming and line-count invariants.

#### Scenario: Unknown task
- **WHEN** the owner resolves but task_id is absent
- **THEN** no task is created and parent active status is returned.

#### Scenario: Child
- **WHEN** the session match is Child and the view exists
- **THEN** that child's central task store is targeted.

#### Scenario: Invalid
- **WHEN** parsing, variant or routing fails
- **THEN** false is returned.

证据：`crates/codegen/pager/src/app/acp_handler/background.rs` — `handle_monitor_event`。

### Requirement: Pager background task completion state and stale-load suppression

A TaskCompleted update SHALL route to the concrete root or child. Success is exit code zero or both exit code and signal absent; signal `session_restart` marks stale-on-load. A known task updates Done/Failed, exit/signal/end time and clears pending kill, deriving elapsed and description from stored state. An unknown task derives elapsed from snapshot times when ordered and a nonblank display_command differing from raw command, stripping a monitor prefix, but is not inserted into the central task map. The original started entry is finished and descriptions synchronize when available. Stale-on-load returns after state finalization without a new terminal block; other completions append completed or failed blocks and return parent active status.

#### Scenario: Known success
- **WHEN** a tracked task completes with exit zero
- **THEN** state becomes Done, kill state clears and a completed block is appended.

#### Scenario: Signal-free unknown exit
- **WHEN** exit code and signal are both absent
- **THEN** completion is classified successful.

#### Scenario: Unknown task
- **WHEN** no central task exists
- **THEN** snapshot data drives a terminal block without inserting central state.

#### Scenario: Description recovery
- **WHEN** stored description is blank but the started block has one
- **THEN** it is recovered into central state and terminal label.

#### Scenario: Stale load
- **WHEN** signal equals session_restart
- **THEN** running state is finalized but no fresh completion/failure block is appended.

#### Scenario: Failure
- **WHEN** success predicate is false
- **THEN** a failed block carries exit code and signal.

证据：`crates/codegen/pager/src/app/acp_handler/background.rs` — `handle_task_completed`。

### Requirement: Shell crates/codegen/shell/src/leader/client.rs leader process and client transport contract

crates/codegen/shell/src/leader/client.rs SHALL 维护 leader process and client transport 的入口 CONNECT_TIMEOUT, RECONNECT_DELAY, MAX_RECONNECT_ATTEMPTS, KEEPALIVE_INTERVAL, REGISTRATION_RESPONSE_TIMEOUT, LEADER_READY_TIMEOUT, DisconnectReason, LeaderRegistration, ControlResponse, LeaderClient, ClientError, connect, shutting_down_reason, send, send_control, recv, registration, cancel (plus 22 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、child process lifecycle；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** CONNECT_TIMEOUT, RECONNECT_DELAY, MAX_RECONNECT_ATTEMPTS, KEEPALIVE_INTERVAL, REGISTRATION_RESPONSE_TIMEOUT, LEADER_READY_TIMEOUT, DisconnectReason, LeaderRegistration, ControlResponse, LeaderClient, ClientError, connect, shutting_down_reason, send, send_control, recv, registration, cancel (plus 22 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** CONNECT_TIMEOUT, RECONNECT_DELAY, MAX_RECONNECT_ATTEMPTS, KEEPALIVE_INTERVAL, REGISTRATION_RESPONSE_TIMEOUT, LEADER_READY_TIMEOUT, DisconnectReason, LeaderRegistration, ControlResponse, LeaderClient, ClientError, connect, shutting_down_reason, send, send_control, recv, registration, cancel (plus 22 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** CONNECT_TIMEOUT, RECONNECT_DELAY, MAX_RECONNECT_ATTEMPTS, KEEPALIVE_INTERVAL, REGISTRATION_RESPONSE_TIMEOUT, LEADER_READY_TIMEOUT, DisconnectReason, LeaderRegistration, ControlResponse, LeaderClient, ClientError, connect, shutting_down_reason, send, send_control, recv, registration, cancel (plus 22 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/leader/client.rs`。

### Requirement: Shell crates/codegen/shell/src/leader/in_process.rs leader process and client transport contract

crates/codegen/shell/src/leader/in_process.rs SHALL 维护 leader process and client transport 的入口 SIMPLEX_BUF, spawn_agent。实现显示该边界包含 filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、child process lifecycle、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** SIMPLEX_BUF, spawn_agent 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** SIMPLEX_BUF, spawn_agent 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** SIMPLEX_BUF, spawn_agent 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/leader/in_process.rs`。

### Requirement: Shell crates/codegen/shell/src/leader/lock.rs leader process and client transport contract

crates/codegen/shell/src/leader/lock.rs SHALL 维护 leader process and client transport 的入口 LEADER_SOCKET_ENV, leader_socket_override, lock_path_for_socket, resolve_socket_path, resolve_lock_path, lock_path_in, lock_path, socket_path_in, socket_path, LockError, LeaderLock, new, open_lock_file, mark_acquired, try_acquire, acquire_blocking, acquire_reopen_timeout, write_pid (plus 30 additional private symbols)。实现显示该边界包含 filesystem or durable record I/O、explicit error/result paths、child process lifecycle、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** LEADER_SOCKET_ENV, leader_socket_override, lock_path_for_socket, resolve_socket_path, resolve_lock_path, lock_path_in, lock_path, socket_path_in, socket_path, LockError, LeaderLock, new, open_lock_file, mark_acquired, try_acquire, acquire_blocking, acquire_reopen_timeout, write_pid (plus 30 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** LEADER_SOCKET_ENV, leader_socket_override, lock_path_for_socket, resolve_socket_path, resolve_lock_path, lock_path_in, lock_path, socket_path_in, socket_path, LockError, LeaderLock, new, open_lock_file, mark_acquired, try_acquire, acquire_blocking, acquire_reopen_timeout, write_pid (plus 30 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/leader/lock.rs`。

### Requirement: Shell crates/codegen/shell/src/leader/mod.rs leader process and client transport contract

crates/codegen/shell/src/leader/mod.rs SHALL 维护 leader process and client transport 的入口 SPAWN_WAIT_TIMEOUT, SPAWN_POLL_INTERVAL, EVICT_WAIT_TIMEOUT, ZOMBIE_EVICT_DEADLINE, leader_is_older_than, RECONNECT_BASE_DELAY, RECONNECT_MAX_DELAY, RECONNECT_MAX_ATTEMPTS_BOUNDED, LeaderDiscoveryState, LeaderTargetErrorCode, fmt, LeaderTargetError, new, LiveLeaderInfo, LeaderDescriptor, LeaderTargetSelection, socket_path, lock_path (plus 98 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、child process lifecycle；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** SPAWN_WAIT_TIMEOUT, SPAWN_POLL_INTERVAL, EVICT_WAIT_TIMEOUT, ZOMBIE_EVICT_DEADLINE, leader_is_older_than, RECONNECT_BASE_DELAY, RECONNECT_MAX_DELAY, RECONNECT_MAX_ATTEMPTS_BOUNDED, LeaderDiscoveryState, LeaderTargetErrorCode, fmt, LeaderTargetError, new, LiveLeaderInfo, LeaderDescriptor, LeaderTargetSelection, socket_path, lock_path (plus 98 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** SPAWN_WAIT_TIMEOUT, SPAWN_POLL_INTERVAL, EVICT_WAIT_TIMEOUT, ZOMBIE_EVICT_DEADLINE, leader_is_older_than, RECONNECT_BASE_DELAY, RECONNECT_MAX_DELAY, RECONNECT_MAX_ATTEMPTS_BOUNDED, LeaderDiscoveryState, LeaderTargetErrorCode, fmt, LeaderTargetError, new, LiveLeaderInfo, LeaderDescriptor, LeaderTargetSelection, socket_path, lock_path (plus 98 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** SPAWN_WAIT_TIMEOUT, SPAWN_POLL_INTERVAL, EVICT_WAIT_TIMEOUT, ZOMBIE_EVICT_DEADLINE, leader_is_older_than, RECONNECT_BASE_DELAY, RECONNECT_MAX_DELAY, RECONNECT_MAX_ATTEMPTS_BOUNDED, LeaderDiscoveryState, LeaderTargetErrorCode, fmt, LeaderTargetError, new, LiveLeaderInfo, LeaderDescriptor, LeaderTargetSelection, socket_path, lock_path (plus 98 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/leader/mod.rs`。

### Requirement: Shell crates/codegen/shell/src/leader/protocol.rs leader process and client transport contract

crates/codegen/shell/src/leader/protocol.rs SHALL 维护 leader process and client transport 的入口 MAX_MESSAGE_SIZE, ProtocolError, read_frame, write_frame, read_message, write_message, ClientId, new, COUNTER, default, ClientMode, LEADER_PROTOCOL_VERSION, ClientCapabilities, ControlCommand, ControlPayload, ClientMessage, ShutdownReason, ServerMessage (plus 12 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、child process lifecycle、platform or feature-gated branches、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** MAX_MESSAGE_SIZE, ProtocolError, read_frame, write_frame, read_message, write_message, ClientId, new, COUNTER, default, ClientMode, LEADER_PROTOCOL_VERSION, ClientCapabilities, ControlCommand, ControlPayload, ClientMessage, ShutdownReason, ServerMessage (plus 12 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** MAX_MESSAGE_SIZE, ProtocolError, read_frame, write_frame, read_message, write_message, ClientId, new, COUNTER, default, ClientMode, LEADER_PROTOCOL_VERSION, ClientCapabilities, ControlCommand, ControlPayload, ClientMessage, ShutdownReason, ServerMessage (plus 12 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/leader/protocol.rs`。

### Requirement: Shell crates/codegen/shell/src/leader/server.rs leader process and client transport contract

crates/codegen/shell/src/leader/server.rs SHALL 维护 leader process and client transport 的入口 LEADER_VERSION, REGISTRATION_TIMEOUT, ID_NAMESPACE_SEP, MAX_BUFFERED_LIVE_PER_LOAD, ServerEvent, LeaderServerPoll, BufferedLive, ClientOutbound, from, ServerMessageRef, write_outbound, ClientState, string, even, LeaderServerMetadata, LeaderServerControlState, new, runtime_cpu_profile (plus 190 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、child process lifecycle；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** LEADER_VERSION, REGISTRATION_TIMEOUT, ID_NAMESPACE_SEP, MAX_BUFFERED_LIVE_PER_LOAD, ServerEvent, LeaderServerPoll, BufferedLive, ClientOutbound, from, ServerMessageRef, write_outbound, ClientState, string, even, LeaderServerMetadata, LeaderServerControlState, new, runtime_cpu_profile (plus 190 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** LEADER_VERSION, REGISTRATION_TIMEOUT, ID_NAMESPACE_SEP, MAX_BUFFERED_LIVE_PER_LOAD, ServerEvent, LeaderServerPoll, BufferedLive, ClientOutbound, from, ServerMessageRef, write_outbound, ClientState, string, even, LeaderServerMetadata, LeaderServerControlState, new, runtime_cpu_profile (plus 190 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** LEADER_VERSION, REGISTRATION_TIMEOUT, ID_NAMESPACE_SEP, MAX_BUFFERED_LIVE_PER_LOAD, ServerEvent, LeaderServerPoll, BufferedLive, ClientOutbound, from, ServerMessageRef, write_outbound, ClientState, string, even, LeaderServerMetadata, LeaderServerControlState, new, runtime_cpu_profile (plus 190 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/leader/server.rs`。

### Requirement: Shell crates/codegen/shell/src/leader/test_support.rs leader process and client transport contract

crates/codegen/shell/src/leader/test_support.rs SHALL 维护 leader process and client transport 的入口 FakeVersions, current, FakeLeaderBehavior, FakeLeaderHandle, cancel, spawn_fake_leader, serve_client, registered。实现显示该边界包含 filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、timeout/deadline or timing decisions、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** FakeVersions, current, FakeLeaderBehavior, FakeLeaderHandle, cancel, spawn_fake_leader, serve_client, registered 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** FakeVersions, current, FakeLeaderBehavior, FakeLeaderHandle, cancel, spawn_fake_leader, serve_client, registered 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** FakeVersions, current, FakeLeaderBehavior, FakeLeaderHandle, cancel, spawn_fake_leader, serve_client, registered 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/leader/test_support.rs`。

### Requirement: Shell crates/codegen/shell/src/terminal/acp_terminal.rs terminal and PTY lifecycle contract

crates/codegen/shell/src/terminal/acp_terminal.rs SHALL 维护 terminal and PTY lifecycle 的入口 AcpTerminalRunner, run。实现显示该边界包含 explicit error/result paths、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** AcpTerminalRunner, run 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/terminal/acp_terminal.rs`。

### Requirement: Shell crates/codegen/shell/src/terminal/adapter.rs terminal and PTY lifecycle contract

crates/codegen/shell/src/terminal/adapter.rs SHALL 维护 terminal and PTY lifecycle 的入口 SnapshotOutput, TrackedTask, default, mark_completed, to_snapshot, TaskMap, wrap_command, to_env, parse_exit, AcpTerminalAdapter, new, create_terminal, terminal_id, run, run_background, get_task, Resolved, kill_task (plus 5 additional private symbols)。实现显示该边界包含 explicit error/result paths、async task lifecycle and cancellation、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection、prompt/subagent/goal context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** SnapshotOutput, TrackedTask, default, mark_completed, to_snapshot, TaskMap, wrap_command, to_env, parse_exit, AcpTerminalAdapter, new, create_terminal, terminal_id, run, run_background, get_task, Resolved, kill_task (plus 5 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** SnapshotOutput, TrackedTask, default, mark_completed, to_snapshot, TaskMap, wrap_command, to_env, parse_exit, AcpTerminalAdapter, new, create_terminal, terminal_id, run, run_background, get_task, Resolved, kill_task (plus 5 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/terminal/adapter.rs`。

### Requirement: Shell crates/codegen/shell/src/terminal/adapter_tests.rs terminal and PTY lifecycle contract

crates/codegen/shell/src/terminal/adapter_tests.rs SHALL 维护 terminal and PTY lifecycle 的入口 make_tracked_task, out, wrap_command_quotes_shell_metacharacters, parse_exit_maps_code_signal_and_none, to_snapshot_derives_completed_and_end_time, scripted_gateway, background_request, run_background_records_snapshots_and_threads_task_kind, output_unavailable_gateway, insert_task, get_task_completed_keeps_completion_buffer_over_log, kill_wait_gateway, str, kill_task_unknown_id_answers_not_found_despite_lenient_client_kill, kill_task_unknown_id_probe_transport_failure_still_kills, kill_task_tracked_running_kills_and_marks_explicitly_killed, kill_task_tracked_completed_answers_already_exited_without_round_trips, kill_task_untracked_live_terminal_still_kills (plus 4 additional private symbols)。实现显示该边界包含 filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、child process lifecycle、platform or feature-gated branches；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** make_tracked_task, out, wrap_command_quotes_shell_metacharacters, parse_exit_maps_code_signal_and_none, to_snapshot_derives_completed_and_end_time, scripted_gateway, background_request, run_background_records_snapshots_and_threads_task_kind, output_unavailable_gateway, insert_task, get_task_completed_keeps_completion_buffer_over_log, kill_wait_gateway, str, kill_task_unknown_id_answers_not_found_despite_lenient_client_kill, kill_task_unknown_id_probe_transport_failure_still_kills, kill_task_tracked_running_kills_and_marks_explicitly_killed, kill_task_tracked_completed_answers_already_exited_without_round_trips, kill_task_untracked_live_terminal_still_kills (plus 4 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** make_tracked_task, out, wrap_command_quotes_shell_metacharacters, parse_exit_maps_code_signal_and_none, to_snapshot_derives_completed_and_end_time, scripted_gateway, background_request, run_background_records_snapshots_and_threads_task_kind, output_unavailable_gateway, insert_task, get_task_completed_keeps_completion_buffer_over_log, kill_wait_gateway, str, kill_task_unknown_id_answers_not_found_despite_lenient_client_kill, kill_task_unknown_id_probe_transport_failure_still_kills, kill_task_tracked_running_kills_and_marks_explicitly_killed, kill_task_tracked_completed_answers_already_exited_without_round_trips, kill_task_untracked_live_terminal_still_kills (plus 4 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** make_tracked_task, out, wrap_command_quotes_shell_metacharacters, parse_exit_maps_code_signal_and_none, to_snapshot_derives_completed_and_end_time, scripted_gateway, background_request, run_background_records_snapshots_and_threads_task_kind, output_unavailable_gateway, insert_task, get_task_completed_keeps_completion_buffer_over_log, kill_wait_gateway, str, kill_task_unknown_id_answers_not_found_despite_lenient_client_kill, kill_task_unknown_id_probe_transport_failure_still_kills, kill_task_tracked_running_kills_and_marks_explicitly_killed, kill_task_tracked_completed_answers_already_exited_without_round_trips, kill_task_untracked_live_terminal_still_kills (plus 4 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/terminal/adapter_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/terminal/exit_watcher.rs terminal and PTY lifecycle contract

crates/codegen/shell/src/terminal/exit_watcher.rs SHALL 维护 terminal and PTY lifecycle 的入口 RECORDER_POLL, EXIT_POLL_INTERVAL, GATEWAY_LOST_AFTER, max_poll_errors, PollStep, poll_terminal_output, Exit, watch_for_exit, complete_and_release, release_terminal, poll_for_terminal_exit。实现显示该边界包含 explicit error/result paths、async task lifecycle and cancellation、child process lifecycle、timeout/deadline or timing decisions、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** RECORDER_POLL, EXIT_POLL_INTERVAL, GATEWAY_LOST_AFTER, max_poll_errors, PollStep, poll_terminal_output, Exit, watch_for_exit, complete_and_release, release_terminal, poll_for_terminal_exit 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** RECORDER_POLL, EXIT_POLL_INTERVAL, GATEWAY_LOST_AFTER, max_poll_errors, PollStep, poll_terminal_output, Exit, watch_for_exit, complete_and_release, release_terminal, poll_for_terminal_exit 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/terminal/exit_watcher.rs`。

### Requirement: Shell crates/codegen/shell/src/terminal/local_terminal.rs terminal and PTY lifecycle contract

crates/codegen/shell/src/terminal/local_terminal.rs SHALL 维护 terminal and PTY lifecycle 的入口 LocalTerminalRunner, read_stream, truncate_buffer, run, make_request, test_child_cannot_open_dev_tty, test_basic_command_output。实现显示该边界包含 filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、child process lifecycle、platform or feature-gated branches、timeout/deadline or timing decisions；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** LocalTerminalRunner, read_stream, truncate_buffer, run, make_request, test_child_cannot_open_dev_tty, test_basic_command_output 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** LocalTerminalRunner, read_stream, truncate_buffer, run, make_request, test_child_cannot_open_dev_tty, test_basic_command_output 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** LocalTerminalRunner, read_stream, truncate_buffer, run, make_request, test_child_cannot_open_dev_tty, test_basic_command_output 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/terminal/local_terminal.rs`。

### Requirement: Shell crates/codegen/shell/src/terminal/mod.rs terminal and PTY lifecycle contract

crates/codegen/shell/src/terminal/mod.rs SHALL 维护 terminal and PTY lifecycle 的入口 DEFAULT_TIMEOUT, DEFAULT_OUTPUT_BYTE_LIMIT, default_shell_path, str, TerminalStatus, TerminalInfo, TerminalExtError, code, terminal_id, from, list_terminals, color_env, no_color_env, TerminalRunner, new, run。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** DEFAULT_TIMEOUT, DEFAULT_OUTPUT_BYTE_LIMIT, default_shell_path, str, TerminalStatus, TerminalInfo, TerminalExtError, code, terminal_id, from, list_terminals, color_env, no_color_env, TerminalRunner, new, run 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/terminal/mod.rs`。

### Requirement: Shell crates/codegen/shell/src/terminal/output_recorder.rs terminal and PTY lifecycle contract

crates/codegen/shell/src/terminal/output_recorder.rs SHALL 维护 terminal and PTY lifecycle 的入口 OutputRecorder, new, mirrored, initialize, append, largest_overlap, LogTail, read_log_tail, ov, largest_overlap_finds_rolling_tail_alignment, recorder_appends_cumulative_suffixes, recorder_retries_the_suffix_after_a_failed_write, recorder_reconstructs_stream_across_repeated_rolls, recorder_ignores_empty_snapshot, read_log_tail_drops_leading_partial_char, read_log_tail_drops_trailing_partial_char。实现显示该边界包含 filesystem or durable record I/O、explicit error/result paths、platform or feature-gated branches、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** OutputRecorder, new, mirrored, initialize, append, largest_overlap, LogTail, read_log_tail, ov, largest_overlap_finds_rolling_tail_alignment, recorder_appends_cumulative_suffixes, recorder_retries_the_suffix_after_a_failed_write, recorder_reconstructs_stream_across_repeated_rolls, recorder_ignores_empty_snapshot, read_log_tail_drops_leading_partial_char, read_log_tail_drops_trailing_partial_char 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** OutputRecorder, new, mirrored, initialize, append, largest_overlap, LogTail, read_log_tail, ov, largest_overlap_finds_rolling_tail_alignment, recorder_appends_cumulative_suffixes, recorder_retries_the_suffix_after_a_failed_write, recorder_reconstructs_stream_across_repeated_rolls, recorder_ignores_empty_snapshot, read_log_tail_drops_leading_partial_char, read_log_tail_drops_trailing_partial_char 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/terminal/output_recorder.rs`。

### Requirement: Shell crates/codegen/shell/src/terminal/pty_session.rs terminal and PTY lifecycle contract

crates/codegen/shell/src/terminal/pty_session.rs SHALL 维护 terminal and PTY lifecycle 的入口 NOTIFICATION_METHOD, OUTPUT_RING_BUFFER_SIZE, OUTPUT_BATCH_INTERVAL_MS, BUSY_POLL_INTERVAL_MS, INPUT_CHANNEL_CAPACITY, EXIT_POLL_INTERVAL, REAP_GRACE, PtySession, Shell, pid, hangup, attach_group, reap_now, wait_exit, kill, poll_exit, exit_code, UnregisteredShell (plus 42 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、child process lifecycle；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** NOTIFICATION_METHOD, OUTPUT_RING_BUFFER_SIZE, OUTPUT_BATCH_INTERVAL_MS, BUSY_POLL_INTERVAL_MS, INPUT_CHANNEL_CAPACITY, EXIT_POLL_INTERVAL, REAP_GRACE, PtySession, Shell, pid, hangup, attach_group, reap_now, wait_exit, kill, poll_exit, exit_code, UnregisteredShell (plus 42 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** NOTIFICATION_METHOD, OUTPUT_RING_BUFFER_SIZE, OUTPUT_BATCH_INTERVAL_MS, BUSY_POLL_INTERVAL_MS, INPUT_CHANNEL_CAPACITY, EXIT_POLL_INTERVAL, REAP_GRACE, PtySession, Shell, pid, hangup, attach_group, reap_now, wait_exit, kill, poll_exit, exit_code, UnregisteredShell (plus 42 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** NOTIFICATION_METHOD, OUTPUT_RING_BUFFER_SIZE, OUTPUT_BATCH_INTERVAL_MS, BUSY_POLL_INTERVAL_MS, INPUT_CHANNEL_CAPACITY, EXIT_POLL_INTERVAL, REAP_GRACE, PtySession, Shell, pid, hangup, attach_group, reap_now, wait_exit, kill, poll_exit, exit_code, UnregisteredShell (plus 42 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/terminal/pty_session.rs`。

### Requirement: Shell crates/codegen/shell/src/terminal/runner.rs terminal and PTY lifecycle contract

crates/codegen/shell/src/terminal/runner.rs SHALL 维护 terminal and PTY lifecycle 的入口 TerminalError, TerminalRunRequest, TerminalRunResult, AsyncTerminalRunner, run。实现显示该边界包含 explicit error/result paths、timeout/deadline or timing decisions、git/worktree context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** TerminalError, TerminalRunRequest, TerminalRunResult, AsyncTerminalRunner, run 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

证据：`crates/codegen/shell/src/terminal/runner.rs`。

### Requirement: Shell crates/codegen/shell/src/terminal/streaming_local_terminal.rs terminal and PTY lifecycle contract

crates/codegen/shell/src/terminal/streaming_local_terminal.rs SHALL 维护 terminal and PTY lifecycle 的入口 DEFAULT_NOTIFICATION_INTERVAL_MS, READ_BUFFER_SIZE, KILL_REAP_TIMEOUT, notification_interval, ExitStatus, OutputSnapshot, KillOutcome, SessionNotificationSender, session_notification, GatedNotifier, new, ChildHandle, OutputState, TerminalEntry, TerminalKey, TerminalMap, TERMINAL_REGISTRY, registry (plus 40 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、child process lifecycle、platform or feature-gated branches；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** DEFAULT_NOTIFICATION_INTERVAL_MS, READ_BUFFER_SIZE, KILL_REAP_TIMEOUT, notification_interval, ExitStatus, OutputSnapshot, KillOutcome, SessionNotificationSender, session_notification, GatedNotifier, new, ChildHandle, OutputState, TerminalEntry, TerminalKey, TerminalMap, TERMINAL_REGISTRY, registry (plus 40 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** DEFAULT_NOTIFICATION_INTERVAL_MS, READ_BUFFER_SIZE, KILL_REAP_TIMEOUT, notification_interval, ExitStatus, OutputSnapshot, KillOutcome, SessionNotificationSender, session_notification, GatedNotifier, new, ChildHandle, OutputState, TerminalEntry, TerminalKey, TerminalMap, TERMINAL_REGISTRY, registry (plus 40 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** DEFAULT_NOTIFICATION_INTERVAL_MS, READ_BUFFER_SIZE, KILL_REAP_TIMEOUT, notification_interval, ExitStatus, OutputSnapshot, KillOutcome, SessionNotificationSender, session_notification, GatedNotifier, new, ChildHandle, OutputState, TerminalEntry, TerminalKey, TerminalMap, TERMINAL_REGISTRY, registry (plus 40 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/terminal/streaming_local_terminal.rs`。
### Requirement: Nix pseudo-terminal lifecycle
PTY wrappers SHALL expose OpenptyResult/ForkptyResult, PtyMaster descriptor ownership adapters, grantpt/posix_openpt/ptsname/ptsname_r/unlockpt/openpty/forkpty, and Read/Write/Flush behavior while preserving the documented unsafe and manual-close boundaries.

#### Scenario: Nix pseudo-terminal lifecycle implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/pty.rs` — `struct OpenptyResult`；`third_party/nix-ohos/src/pty.rs` — `struct ForkptyResult`；`third_party/nix-ohos/src/pty.rs` — `struct PtyMaster`；`third_party/nix-ohos/src/pty.rs` — `fn as_raw_fd`；`third_party/nix-ohos/src/pty.rs` — `fn into_raw_fd`；`third_party/nix-ohos/src/pty.rs` — `fn drop`；`third_party/nix-ohos/src/pty.rs` — `fn read`；`third_party/nix-ohos/src/pty.rs` — `fn write`；`third_party/nix-ohos/src/pty.rs` — `fn flush`；`third_party/nix-ohos/src/pty.rs` — `fn read`；`third_party/nix-ohos/src/pty.rs` — `fn write`；`third_party/nix-ohos/src/pty.rs` — `fn flush`；`third_party/nix-ohos/src/pty.rs` — `fn grantpt`；`third_party/nix-ohos/src/pty.rs` — `fn posix_openpt`；`third_party/nix-ohos/src/pty.rs` — `fn ptsname`；`third_party/nix-ohos/src/pty.rs` — `fn ptsname_r`；`third_party/nix-ohos/src/pty.rs` — `fn unlockpt`；`third_party/nix-ohos/src/pty.rs` — `fn openpty`；`third_party/nix-ohos/src/pty.rs` — `fn forkpty`。

### Requirement: Nix process scheduling and namespaces
Scheduling wrappers SHALL expose CloneFlags/CloneCb and clone/unshare/setns, provide CpuSet construction/mutation/count and affinity accessors, and wrap sched_getcpu/sched_yield with typed return errors under Linux-like targets.

#### Scenario: Nix process scheduling and namespaces implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/sched.rs` — `struct CloneFlags`；`third_party/nix-ohos/src/sched.rs` — `type CloneCb`；`third_party/nix-ohos/src/sched.rs` — `fn clone`；`third_party/nix-ohos/src/sched.rs` — `fn callback`；`third_party/nix-ohos/src/sched.rs` — `fn unshare`；`third_party/nix-ohos/src/sched.rs` — `fn setns`；`third_party/nix-ohos/src/sched.rs` — `struct CpuSet`；`third_party/nix-ohos/src/sched.rs` — `fn new`；`third_party/nix-ohos/src/sched.rs` — `fn is_set`；`third_party/nix-ohos/src/sched.rs` — `fn set`；`third_party/nix-ohos/src/sched.rs` — `fn unset`；`third_party/nix-ohos/src/sched.rs` — `fn count`；`third_party/nix-ohos/src/sched.rs` — `fn default`；`third_party/nix-ohos/src/sched.rs` — `fn sched_setaffinity`；`third_party/nix-ohos/src/sched.rs` — `fn sched_getaffinity`；`third_party/nix-ohos/src/sched.rs` — `fn sched_getcpu`；`third_party/nix-ohos/src/sched.rs` — `fn sched_yield`。

### Requirement: Nix POSIX asynchronous I/O
AIO SHALL model asynchronous read/write/fsync/readv/writev requests, completion notification and cancellation states, submit/suspend/list operations, ownership/drop behavior, and typed conversion of libc aiocb results under platform cfgs.

#### Scenario: test:casting
- **WHEN** the embedded test function casting is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

#### Scenario: test:casting_vectored
- **WHEN** the embedded test function casting_vectored is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

证据：`third_party/nix-ohos/src/sys/aio.rs` — `enum AioFsyncMode`；`third_party/nix-ohos/src/sys/aio.rs` — `enum LioMode`；`third_party/nix-ohos/src/sys/aio.rs` — `enum AioCancelStat`；`third_party/nix-ohos/src/sys/aio.rs` — `struct LibcAiocb`；`third_party/nix-ohos/src/sys/aio.rs` — `struct AioCb`；`third_party/nix-ohos/src/sys/aio.rs` — `fn aio_return`；`third_party/nix-ohos/src/sys/aio.rs` — `fn cancel`；`third_party/nix-ohos/src/sys/aio.rs` — `fn common_init`；`third_party/nix-ohos/src/sys/aio.rs` — `fn error`；`third_party/nix-ohos/src/sys/aio.rs` — `fn in_progress`；`third_party/nix-ohos/src/sys/aio.rs` — `fn set_in_progress`；`third_party/nix-ohos/src/sys/aio.rs` — `fn set_sigev_notify`；`third_party/nix-ohos/src/sys/aio.rs` — `fn fmt`；`third_party/nix-ohos/src/sys/aio.rs` — `fn drop`；`third_party/nix-ohos/src/sys/aio.rs` — `trait Aio`；`third_party/nix-ohos/src/sys/aio.rs` — `type Output`；`third_party/nix-ohos/src/sys/aio.rs` — `fn aio_return`；`third_party/nix-ohos/src/sys/aio.rs` — `fn cancel`；`third_party/nix-ohos/src/sys/aio.rs` — `fn error`；`third_party/nix-ohos/src/sys/aio.rs` — `fn fd`；`third_party/nix-ohos/src/sys/aio.rs` — `fn in_progress`；`third_party/nix-ohos/src/sys/aio.rs` — `fn priority`；`third_party/nix-ohos/src/sys/aio.rs` — `fn set_sigev_notify`；`third_party/nix-ohos/src/sys/aio.rs` — `fn sigevent`；`third_party/nix-ohos/src/sys/aio.rs` — `fn submit`；`third_party/nix-ohos/src/sys/aio.rs` — `macro aio_methods`；`third_party/nix-ohos/src/sys/aio.rs` — `fn cancel`；`third_party/nix-ohos/src/sys/aio.rs` — `fn error`；`third_party/nix-ohos/src/sys/aio.rs` — `fn fd`；`third_party/nix-ohos/src/sys/aio.rs` — `fn in_progress`；`third_party/nix-ohos/src/sys/aio.rs` — `fn priority`；`third_party/nix-ohos/src/sys/aio.rs` — `fn set_sigev_notify`；`third_party/nix-ohos/src/sys/aio.rs` — `fn sigevent`；`third_party/nix-ohos/src/sys/aio.rs` — `fn aio_return`；`third_party/nix-ohos/src/sys/aio.rs` — `fn submit`；`third_party/nix-ohos/src/sys/aio.rs` — `struct AioFsync`；`third_party/nix-ohos/src/sys/aio.rs` — `fn mode`；`third_party/nix-ohos/src/sys/aio.rs` — `fn new`；`third_party/nix-ohos/src/sys/aio.rs` — `type Output`；`third_party/nix-ohos/src/sys/aio.rs` — `fn aio_return`；`third_party/nix-ohos/src/sys/aio.rs` — `fn submit`；`third_party/nix-ohos/src/sys/aio.rs` — `fn as_ref`；`third_party/nix-ohos/src/sys/aio.rs` — `struct AioRead`；`third_party/nix-ohos/src/sys/aio.rs` — `fn nbytes`；`third_party/nix-ohos/src/sys/aio.rs` — `fn new`；`third_party/nix-ohos/src/sys/aio.rs` — `fn offset`；`third_party/nix-ohos/src/sys/aio.rs` — `type Output`；`third_party/nix-ohos/src/sys/aio.rs` — `fn as_mut`；`third_party/nix-ohos/src/sys/aio.rs` — `fn as_ref`；`third_party/nix-ohos/src/sys/aio.rs` — `struct AioReadv`；`third_party/nix-ohos/src/sys/aio.rs` — `fn iovlen`；`third_party/nix-ohos/src/sys/aio.rs` — `fn new`；`third_party/nix-ohos/src/sys/aio.rs` — `fn offset`；`third_party/nix-ohos/src/sys/aio.rs` — `type Output`；`third_party/nix-ohos/src/sys/aio.rs` — `fn as_mut`；`third_party/nix-ohos/src/sys/aio.rs` — `fn as_ref`；`third_party/nix-ohos/src/sys/aio.rs` — `struct AioWrite`；`third_party/nix-ohos/src/sys/aio.rs` — `fn nbytes`；`third_party/nix-ohos/src/sys/aio.rs` — `fn new`；`third_party/nix-ohos/src/sys/aio.rs` — `fn offset`；`third_party/nix-ohos/src/sys/aio.rs` — `type Output`；`third_party/nix-ohos/src/sys/aio.rs` — `fn as_mut`；`third_party/nix-ohos/src/sys/aio.rs` — `fn as_ref`；`third_party/nix-ohos/src/sys/aio.rs` — `struct AioWritev`；`third_party/nix-ohos/src/sys/aio.rs` — `fn iovlen`；`third_party/nix-ohos/src/sys/aio.rs` — `fn new`；`third_party/nix-ohos/src/sys/aio.rs` — `fn offset`；`third_party/nix-ohos/src/sys/aio.rs` — `type Output`；`third_party/nix-ohos/src/sys/aio.rs` — `fn as_mut`；`third_party/nix-ohos/src/sys/aio.rs` — `fn as_ref`；`third_party/nix-ohos/src/sys/aio.rs` — `fn aio_cancel_all`；`third_party/nix-ohos/src/sys/aio.rs` — `fn aio_suspend`；`third_party/nix-ohos/src/sys/aio.rs` — `fn lio_listio`；`third_party/nix-ohos/src/sys/aio.rs` — `fn casting`；`third_party/nix-ohos/src/sys/aio.rs` — `fn casting_vectored`。

### Requirement: Nix Linux execution personality
personality SHALL model Linux execution-domain flags and wrap get/set personality calls with cfg-dependent flag representation and Errno conversion.

#### Scenario: Nix Linux execution personality implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/sys/personality.rs` — `struct Persona`；`third_party/nix-ohos/src/sys/personality.rs` — `fn get`；`third_party/nix-ohos/src/sys/personality.rs` — `fn set`。

### Requirement: Nix pthread identity and signals
pthread SHALL expose Pthread and wrap pthread_self plus optional pthread_kill signal delivery/checking, returning typed Errno values.

#### Scenario: Nix pthread identity and signals implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/sys/pthread.rs` — `type Pthread`；`third_party/nix-ohos/src/sys/pthread.rs` — `fn pthread_self`；`third_party/nix-ohos/src/sys/pthread.rs` — `fn pthread_kill`。

### Requirement: Nix BSD ptrace operations
The BSD ptrace backend SHALL model request/address types and wrap trace-me, attach, detach, continue, kill, step, read, and write operations under BSD target cfgs.

#### Scenario: Nix BSD ptrace operations implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/sys/ptrace/bsd.rs` — `type RequestType`；`third_party/nix-ohos/src/sys/ptrace/bsd.rs` — `type AddressType`；`third_party/nix-ohos/src/sys/ptrace/bsd.rs` — `type AddressType`；`third_party/nix-ohos/src/sys/ptrace/bsd.rs` — `enum Request`；`third_party/nix-ohos/src/sys/ptrace/bsd.rs` — `fn ptrace_other`；`third_party/nix-ohos/src/sys/ptrace/bsd.rs` — `fn traceme`；`third_party/nix-ohos/src/sys/ptrace/bsd.rs` — `fn attach`；`third_party/nix-ohos/src/sys/ptrace/bsd.rs` — `fn detach`；`third_party/nix-ohos/src/sys/ptrace/bsd.rs` — `fn cont`；`third_party/nix-ohos/src/sys/ptrace/bsd.rs` — `fn kill`；`third_party/nix-ohos/src/sys/ptrace/bsd.rs` — `fn step`；`third_party/nix-ohos/src/sys/ptrace/bsd.rs` — `fn read`；`third_party/nix-ohos/src/sys/ptrace/bsd.rs` — `fn write`。

### Requirement: Nix Linux ptrace operations
The Linux ptrace backend SHALL model requests/options/events and wrap register/data access, trace/attach/seize/detach/continue/interrupt/kill/step/syscall/sysemu, signal info, and event decoding with OHOS-as-musl cfg handling.

#### Scenario: Nix Linux ptrace operations implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `type AddressType`；`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `type RequestType`；`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `type RequestType`；`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `enum Request`；`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `enum Event`；`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `struct Options`；`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `fn ptrace_peek`；`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `fn getregs`；`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `fn setregs`；`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `fn ptrace_get_data`；`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `fn ptrace_other`；`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `fn setoptions`；`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `fn getevent`；`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `fn getsiginfo`；`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `fn setsiginfo`；`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `fn traceme`；`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `fn syscall`；`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `fn sysemu`；`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `fn attach`；`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `fn seize`；`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `fn detach`；`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `fn cont`；`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `fn interrupt`；`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `fn kill`；`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `fn step`；`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `fn sysemu_step`；`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `fn read`；`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `fn write`；`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `fn read_user`；`third_party/nix-ohos/src/sys/ptrace/linux.rs` — `fn write_user`。

### Requirement: Nix platform ptrace module selection
The ptrace module SHALL re-export the Linux or BSD backend only for declared targets and preserve platform-specific request APIs.

#### Scenario: Nix platform ptrace module selection implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/sys/ptrace/mod.rs` — `mod linux`；`third_party/nix-ohos/src/sys/ptrace/mod.rs` — `pub use self::linux::*`；`third_party/nix-ohos/src/sys/ptrace/mod.rs` — `mod bsd`；`third_party/nix-ohos/src/sys/ptrace/mod.rs` — `pub use self::bsd::*`。

### Requirement: Nix Linux reboot control
reboot SHALL model reboot modes and wrap reboot plus Ctrl-Alt-Delete enable/disable through libc with typed Errno results.

#### Scenario: Nix Linux reboot control implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/sys/reboot.rs` — `enum RebootMode`；`third_party/nix-ohos/src/sys/reboot.rs` — `fn reboot`；`third_party/nix-ohos/src/sys/reboot.rs` — `fn set_cad_enabled`。

### Requirement: Nix process resource limits and usage
resource SHALL model platform Resource/UsageWho/Usage values, wrap getrlimit/setrlimit/getrusage, expose typed time/memory/context metrics, and preserve RLIM_INFINITY and libc conversion behavior.

#### Scenario: test:test_self_cpu_time
- **WHEN** the embedded test function test_self_cpu_time is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

证据：`third_party/nix-ohos/src/sys/resource.rs` — `enum Resource`；`third_party/nix-ohos/src/sys/resource.rs` — `fn getrlimit`；`third_party/nix-ohos/src/sys/resource.rs` — `fn setrlimit`；`third_party/nix-ohos/src/sys/resource.rs` — `enum UsageWho`；`third_party/nix-ohos/src/sys/resource.rs` — `struct Usage`；`third_party/nix-ohos/src/sys/resource.rs` — `fn as_ref`；`third_party/nix-ohos/src/sys/resource.rs` — `fn as_mut`；`third_party/nix-ohos/src/sys/resource.rs` — `fn user_time`；`third_party/nix-ohos/src/sys/resource.rs` — `fn system_time`；`third_party/nix-ohos/src/sys/resource.rs` — `fn max_rss`；`third_party/nix-ohos/src/sys/resource.rs` — `fn shared_integral`；`third_party/nix-ohos/src/sys/resource.rs` — `fn unshared_data_integral`；`third_party/nix-ohos/src/sys/resource.rs` — `fn unshared_stack_integral`；`third_party/nix-ohos/src/sys/resource.rs` — `fn minor_page_faults`；`third_party/nix-ohos/src/sys/resource.rs` — `fn major_page_faults`；`third_party/nix-ohos/src/sys/resource.rs` — `fn full_swaps`；`third_party/nix-ohos/src/sys/resource.rs` — `fn block_reads`；`third_party/nix-ohos/src/sys/resource.rs` — `fn block_writes`；`third_party/nix-ohos/src/sys/resource.rs` — `fn ipc_sends`；`third_party/nix-ohos/src/sys/resource.rs` — `fn ipc_receives`；`third_party/nix-ohos/src/sys/resource.rs` — `fn signals`；`third_party/nix-ohos/src/sys/resource.rs` — `fn voluntary_context_switches`；`third_party/nix-ohos/src/sys/resource.rs` — `fn involuntary_context_switches`；`third_party/nix-ohos/src/sys/resource.rs` — `fn getrusage`；`third_party/nix-ohos/src/sys/resource.rs` — `fn test_self_cpu_time`。

### Requirement: Nix signal sets and handlers
Signal APIs SHALL model platform signal constants, handlers, masks, actions, queues, waits, sender operations, iterators, siginfo and sigevent variants, converting signal-set operations and libc errors under feature/target cfgs.

#### Scenario: test:test_contains
- **WHEN** the embedded test function test_contains is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

#### Scenario: test:test_clear
- **WHEN** the embedded test function test_clear is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

#### Scenario: test:test_from_str_round_trips
- **WHEN** the embedded test function test_from_str_round_trips is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

证据：`third_party/nix-ohos/src/sys/signal.rs` — `enum Signal`；`third_party/nix-ohos/src/sys/signal.rs` — `type Err`；`third_party/nix-ohos/src/sys/signal.rs` — `fn from_str`；`third_party/nix-ohos/src/sys/signal.rs` — `fn as_str`；`third_party/nix-ohos/src/sys/signal.rs` — `fn as_ref`；`third_party/nix-ohos/src/sys/signal.rs` — `fn fmt`；`third_party/nix-ohos/src/sys/signal.rs` — `const SIGNALS`；`third_party/nix-ohos/src/sys/signal.rs` — `const SIGNALS`；`third_party/nix-ohos/src/sys/signal.rs` — `const SIGNALS`；`third_party/nix-ohos/src/sys/signal.rs` — `const SIGNALS`；`third_party/nix-ohos/src/sys/signal.rs` — `const SIGNALS`；`third_party/nix-ohos/src/sys/signal.rs` — `struct SignalIterator`；`third_party/nix-ohos/src/sys/signal.rs` — `type Item`；`third_party/nix-ohos/src/sys/signal.rs` — `fn next`；`third_party/nix-ohos/src/sys/signal.rs` — `fn iterator`；`third_party/nix-ohos/src/sys/signal.rs` — `const SIGIOT`；`third_party/nix-ohos/src/sys/signal.rs` — `const SIGPOLL`；`third_party/nix-ohos/src/sys/signal.rs` — `const SIGUNUSED`；`third_party/nix-ohos/src/sys/signal.rs` — `type SaFlags_t`；`third_party/nix-ohos/src/sys/signal.rs` — `type SaFlags_t`；`third_party/nix-ohos/src/sys/signal.rs` — `type SaFlags_t`；`third_party/nix-ohos/src/sys/signal.rs` — `struct SaFlags`；`third_party/nix-ohos/src/sys/signal.rs` — `enum SigmaskHow`；`third_party/nix-ohos/src/sys/signal.rs` — `struct SigSet`；`third_party/nix-ohos/src/sys/signal.rs` — `fn all`；`third_party/nix-ohos/src/sys/signal.rs` — `fn empty`；`third_party/nix-ohos/src/sys/signal.rs` — `fn add`；`third_party/nix-ohos/src/sys/signal.rs` — `fn clear`；`third_party/nix-ohos/src/sys/signal.rs` — `fn remove`；`third_party/nix-ohos/src/sys/signal.rs` — `fn contains`；`third_party/nix-ohos/src/sys/signal.rs` — `fn iter`；`third_party/nix-ohos/src/sys/signal.rs` — `fn thread_get_mask`；`third_party/nix-ohos/src/sys/signal.rs` — `fn thread_set_mask`；`third_party/nix-ohos/src/sys/signal.rs` — `fn thread_block`；`third_party/nix-ohos/src/sys/signal.rs` — `fn thread_unblock`；`third_party/nix-ohos/src/sys/signal.rs` — `fn thread_swap_mask`；`third_party/nix-ohos/src/sys/signal.rs` — `fn wait`；`third_party/nix-ohos/src/sys/signal.rs` — `fn from_sigset_t_unchecked`；`third_party/nix-ohos/src/sys/signal.rs` — `fn as_ref`；`third_party/nix-ohos/src/sys/signal.rs` — `fn extend`；`third_party/nix-ohos/src/sys/signal.rs` — `fn from_iter`；`third_party/nix-ohos/src/sys/signal.rs` — `struct SigSetIter`；`third_party/nix-ohos/src/sys/signal.rs` — `type Item`；`third_party/nix-ohos/src/sys/signal.rs` — `fn next`；`third_party/nix-ohos/src/sys/signal.rs` — `type Item`；`third_party/nix-ohos/src/sys/signal.rs` — `type IntoIter`；`third_party/nix-ohos/src/sys/signal.rs` — `fn into_iter`；`third_party/nix-ohos/src/sys/signal.rs` — `enum SigHandler`；`third_party/nix-ohos/src/sys/signal.rs` — `struct SigAction`；`third_party/nix-ohos/src/sys/signal.rs` — `fn new`；`third_party/nix-ohos/src/sys/signal.rs` — `fn install_sig`；`third_party/nix-ohos/src/sys/signal.rs` — `fn flags`；`third_party/nix-ohos/src/sys/signal.rs` — `fn mask`；`third_party/nix-ohos/src/sys/signal.rs` — `fn handler`；`third_party/nix-ohos/src/sys/signal.rs` — `fn sigaction`；`third_party/nix-ohos/src/sys/signal.rs` — `fn signal`；`third_party/nix-ohos/src/sys/signal.rs` — `fn do_pthread_sigmask`；`third_party/nix-ohos/src/sys/signal.rs` — `fn pthread_sigmask`；`third_party/nix-ohos/src/sys/signal.rs` — `fn sigprocmask`；`third_party/nix-ohos/src/sys/signal.rs` — `fn kill`；`third_party/nix-ohos/src/sys/signal.rs` — `fn killpg`；`third_party/nix-ohos/src/sys/signal.rs` — `fn raise`；`third_party/nix-ohos/src/sys/signal.rs` — `type type_of_thread_id`；`third_party/nix-ohos/src/sys/signal.rs` — `type type_of_thread_id`；`third_party/nix-ohos/src/sys/signal.rs` — `enum SigevNotify`；`third_party/nix-ohos/src/sys/signal.rs` — `struct SigEvent`；`third_party/nix-ohos/src/sys/signal.rs` — `fn new`；`third_party/nix-ohos/src/sys/signal.rs` — `fn set_tid`；`third_party/nix-ohos/src/sys/signal.rs` — `fn set_tid`；`third_party/nix-ohos/src/sys/signal.rs` — `fn sigevent`；`third_party/nix-ohos/src/sys/signal.rs` — `fn as_mut_ptr`；`third_party/nix-ohos/src/sys/signal.rs` — `fn from`；`third_party/nix-ohos/src/sys/signal.rs` — `fn test_sigaction_handler`；`third_party/nix-ohos/src/sys/signal.rs` — `fn test_sigaction_action`；`third_party/nix-ohos/src/sys/signal.rs` — `fn test_contains`；`third_party/nix-ohos/src/sys/signal.rs` — `fn test_clear`；`third_party/nix-ohos/src/sys/signal.rs` — `fn test_from_str_round_trips`；`third_party/nix-ohos/src/sys/signal.rs` — `fn test_from_str_invalid_value`；`third_party/nix-ohos/src/sys/signal.rs` — `fn test_extend`；`third_party/nix-ohos/src/sys/signal.rs` — `fn test_thread_signal_set_mask`；`third_party/nix-ohos/src/sys/signal.rs` — `fn test_thread_signal_block`；`third_party/nix-ohos/src/sys/signal.rs` — `fn test_thread_signal_unblock`；`third_party/nix-ohos/src/sys/signal.rs` — `fn test_thread_signal_swap`；`third_party/nix-ohos/src/sys/signal.rs` — `fn test_from_and_into_iterator`；`third_party/nix-ohos/src/sys/signal.rs` — `fn test_sigaction`；`third_party/nix-ohos/src/sys/signal.rs` — `fn test_sigwait`；`third_party/nix-ohos/src/sys/signal.rs` — `fn test_from_sigset_t_unchecked`。

### Requirement: Nix Linux signalfd stream
SignalFd SHALL create/configure a signalfd from SigSet/SfdFlags, read siginfo records as an iterator, expose raw descriptors and ownership conversion, and document signal coalescing/drop semantics.

#### Scenario: test:create_signalfd
- **WHEN** the embedded test function create_signalfd is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

#### Scenario: test:create_signalfd_with_opts
- **WHEN** the embedded test function create_signalfd_with_opts is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

#### Scenario: test:read_empty_signalfd
- **WHEN** the embedded test function read_empty_signalfd is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

证据：`third_party/nix-ohos/src/sys/signalfd.rs` — `struct SfdFlags`；`third_party/nix-ohos/src/sys/signalfd.rs` — `const SIGNALFD_NEW`；`third_party/nix-ohos/src/sys/signalfd.rs` — `const SIGNALFD_SIGINFO_SIZE`；`third_party/nix-ohos/src/sys/signalfd.rs` — `fn signalfd`；`third_party/nix-ohos/src/sys/signalfd.rs` — `struct SignalFd`；`third_party/nix-ohos/src/sys/signalfd.rs` — `fn new`；`third_party/nix-ohos/src/sys/signalfd.rs` — `fn with_flags`；`third_party/nix-ohos/src/sys/signalfd.rs` — `fn set_mask`；`third_party/nix-ohos/src/sys/signalfd.rs` — `fn read_signal`；`third_party/nix-ohos/src/sys/signalfd.rs` — `fn drop`；`third_party/nix-ohos/src/sys/signalfd.rs` — `fn as_raw_fd`；`third_party/nix-ohos/src/sys/signalfd.rs` — `type Item`；`third_party/nix-ohos/src/sys/signalfd.rs` — `fn next`；`third_party/nix-ohos/src/sys/signalfd.rs` — `fn create_signalfd`；`third_party/nix-ohos/src/sys/signalfd.rs` — `fn create_signalfd_with_opts`；`third_party/nix-ohos/src/sys/signalfd.rs` — `fn read_empty_signalfd`。

### Requirement: Nix time specification and timer primitives
Time APIs SHALL model TimeSpec/TimeVal and conversion/arithmetic/ordering/formatting, timer expiration and flags, clock/timer syscall wrappers, timeval-like constructors/accessors, and OHOS-as-musl deprecation/layout cfgs.

#### Scenario: test:test_timespec
- **WHEN** the embedded test function test_timespec is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

#### Scenario: test:test_timespec_from
- **WHEN** the embedded test function test_timespec_from is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

#### Scenario: test:test_timespec_neg
- **WHEN** the embedded test function test_timespec_neg is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

证据：`third_party/nix-ohos/src/sys/time.rs` — `fn zero_init_timespec`；`third_party/nix-ohos/src/sys/time.rs` — `struct TimerSpec`；`third_party/nix-ohos/src/sys/time.rs` — `fn none`；`third_party/nix-ohos/src/sys/time.rs` — `fn as_mut`；`third_party/nix-ohos/src/sys/time.rs` — `fn as_ref`；`third_party/nix-ohos/src/sys/time.rs` — `fn from`；`third_party/nix-ohos/src/sys/time.rs` — `enum Expiration`；`third_party/nix-ohos/src/sys/time.rs` — `struct TimerSetTimeFlags`；`third_party/nix-ohos/src/sys/time.rs` — `const TFD_TIMER_ABSTIME`；`third_party/nix-ohos/src/sys/time.rs` — `struct TimerSetTimeFlags`；`third_party/nix-ohos/src/sys/time.rs` — `const TFD_TIMER_ABSTIME`；`third_party/nix-ohos/src/sys/time.rs` — `fn from`；`third_party/nix-ohos/src/sys/time.rs` — `trait TimeValLike`；`third_party/nix-ohos/src/sys/time.rs` — `fn zero`；`third_party/nix-ohos/src/sys/time.rs` — `fn hours`；`third_party/nix-ohos/src/sys/time.rs` — `fn minutes`；`third_party/nix-ohos/src/sys/time.rs` — `fn seconds`；`third_party/nix-ohos/src/sys/time.rs` — `fn milliseconds`；`third_party/nix-ohos/src/sys/time.rs` — `fn microseconds`；`third_party/nix-ohos/src/sys/time.rs` — `fn nanoseconds`；`third_party/nix-ohos/src/sys/time.rs` — `fn num_hours`；`third_party/nix-ohos/src/sys/time.rs` — `fn num_minutes`；`third_party/nix-ohos/src/sys/time.rs` — `fn num_seconds`；`third_party/nix-ohos/src/sys/time.rs` — `fn num_milliseconds`；`third_party/nix-ohos/src/sys/time.rs` — `fn num_microseconds`；`third_party/nix-ohos/src/sys/time.rs` — `fn num_nanoseconds`；`third_party/nix-ohos/src/sys/time.rs` — `struct TimeSpec`；`third_party/nix-ohos/src/sys/time.rs` — `const NANOS_PER_SEC`；`third_party/nix-ohos/src/sys/time.rs` — `const SECS_PER_MINUTE`；`third_party/nix-ohos/src/sys/time.rs` — `const SECS_PER_HOUR`；`third_party/nix-ohos/src/sys/time.rs` — `const TS_MAX_SECONDS`；`third_party/nix-ohos/src/sys/time.rs` — `const TS_MAX_SECONDS`；`third_party/nix-ohos/src/sys/time.rs` — `const TS_MIN_SECONDS`；`third_party/nix-ohos/src/sys/time.rs` — `type timespec_tv_nsec_t`；`third_party/nix-ohos/src/sys/time.rs` — `type timespec_tv_nsec_t`；`third_party/nix-ohos/src/sys/time.rs` — `fn from`；`third_party/nix-ohos/src/sys/time.rs` — `fn from`；`third_party/nix-ohos/src/sys/time.rs` — `fn from`；`third_party/nix-ohos/src/sys/time.rs` — `fn as_ref`；`third_party/nix-ohos/src/sys/time.rs` — `fn as_mut`；`third_party/nix-ohos/src/sys/time.rs` — `fn cmp`；`third_party/nix-ohos/src/sys/time.rs` — `fn partial_cmp`；`third_party/nix-ohos/src/sys/time.rs` — `fn seconds`；`third_party/nix-ohos/src/sys/time.rs` — `fn milliseconds`；`third_party/nix-ohos/src/sys/time.rs` — `fn microseconds`；`third_party/nix-ohos/src/sys/time.rs` — `fn nanoseconds`；`third_party/nix-ohos/src/sys/time.rs` — `fn num_seconds`；`third_party/nix-ohos/src/sys/time.rs` — `fn num_milliseconds`；`third_party/nix-ohos/src/sys/time.rs` — `fn num_microseconds`；`third_party/nix-ohos/src/sys/time.rs` — `fn num_nanoseconds`；`third_party/nix-ohos/src/sys/time.rs` — `fn new`；`third_party/nix-ohos/src/sys/time.rs` — `fn nanos_mod_sec`；`third_party/nix-ohos/src/sys/time.rs` — `fn tv_sec`；`third_party/nix-ohos/src/sys/time.rs` — `fn tv_nsec`；`third_party/nix-ohos/src/sys/time.rs` — `fn from_duration`；`third_party/nix-ohos/src/sys/time.rs` — `fn from_timespec`；`third_party/nix-ohos/src/sys/time.rs` — `type Output`；`third_party/nix-ohos/src/sys/time.rs` — `fn neg`；`third_party/nix-ohos/src/sys/time.rs` — `type Output`；`third_party/nix-ohos/src/sys/time.rs` — `fn add`；`third_party/nix-ohos/src/sys/time.rs` — `type Output`；`third_party/nix-ohos/src/sys/time.rs` — `fn sub`；`third_party/nix-ohos/src/sys/time.rs` — `type Output`；`third_party/nix-ohos/src/sys/time.rs` — `fn mul`；`third_party/nix-ohos/src/sys/time.rs` — `type Output`；`third_party/nix-ohos/src/sys/time.rs` — `fn div`；`third_party/nix-ohos/src/sys/time.rs` — `fn fmt`；`third_party/nix-ohos/src/sys/time.rs` — `struct TimeVal`；`third_party/nix-ohos/src/sys/time.rs` — `const MICROS_PER_SEC`；`third_party/nix-ohos/src/sys/time.rs` — `const TV_MAX_SECONDS`；`third_party/nix-ohos/src/sys/time.rs` — `const TV_MAX_SECONDS`；`third_party/nix-ohos/src/sys/time.rs` — `const TV_MIN_SECONDS`；`third_party/nix-ohos/src/sys/time.rs` — `fn as_ref`；`third_party/nix-ohos/src/sys/time.rs` — `fn as_mut`；`third_party/nix-ohos/src/sys/time.rs` — `fn cmp`；`third_party/nix-ohos/src/sys/time.rs` — `fn partial_cmp`；`third_party/nix-ohos/src/sys/time.rs` — `fn seconds`；`third_party/nix-ohos/src/sys/time.rs` — `fn milliseconds`；`third_party/nix-ohos/src/sys/time.rs` — `fn microseconds`；`third_party/nix-ohos/src/sys/time.rs` — `fn nanoseconds`；`third_party/nix-ohos/src/sys/time.rs` — `fn num_seconds`；`third_party/nix-ohos/src/sys/time.rs` — `fn num_milliseconds`；`third_party/nix-ohos/src/sys/time.rs` — `fn num_microseconds`；`third_party/nix-ohos/src/sys/time.rs` — `fn num_nanoseconds`；`third_party/nix-ohos/src/sys/time.rs` — `fn new`；`third_party/nix-ohos/src/sys/time.rs` — `fn micros_mod_sec`；`third_party/nix-ohos/src/sys/time.rs` — `fn tv_sec`；`third_party/nix-ohos/src/sys/time.rs` — `fn tv_usec`；`third_party/nix-ohos/src/sys/time.rs` — `type Output`；`third_party/nix-ohos/src/sys/time.rs` — `fn neg`；`third_party/nix-ohos/src/sys/time.rs` — `type Output`；`third_party/nix-ohos/src/sys/time.rs` — `fn add`；`third_party/nix-ohos/src/sys/time.rs` — `type Output`；`third_party/nix-ohos/src/sys/time.rs` — `fn sub`；`third_party/nix-ohos/src/sys/time.rs` — `type Output`；`third_party/nix-ohos/src/sys/time.rs` — `fn mul`；`third_party/nix-ohos/src/sys/time.rs` — `type Output`；`third_party/nix-ohos/src/sys/time.rs` — `fn div`；`third_party/nix-ohos/src/sys/time.rs` — `fn fmt`；`third_party/nix-ohos/src/sys/time.rs` — `fn from`；`third_party/nix-ohos/src/sys/time.rs` — `fn div_mod_floor_64`；`third_party/nix-ohos/src/sys/time.rs` — `fn div_floor_64`；`third_party/nix-ohos/src/sys/time.rs` — `fn mod_floor_64`；`third_party/nix-ohos/src/sys/time.rs` — `fn div_rem_64`；`third_party/nix-ohos/src/sys/time.rs` — `fn test_timespec`；`third_party/nix-ohos/src/sys/time.rs` — `fn test_timespec_from`；`third_party/nix-ohos/src/sys/time.rs` — `fn test_timespec_neg`；`third_party/nix-ohos/src/sys/time.rs` — `fn test_timespec_ord`；`third_party/nix-ohos/src/sys/time.rs` — `fn test_timespec_fmt`；`third_party/nix-ohos/src/sys/time.rs` — `fn test_timeval`；`third_party/nix-ohos/src/sys/time.rs` — `fn test_timeval_ord`；`third_party/nix-ohos/src/sys/time.rs` — `fn test_timeval_neg`；`third_party/nix-ohos/src/sys/time.rs` — `fn test_timeval_fmt`。

### Requirement: Nix POSIX signal timer lifecycle
Timer SHALL create/delete/set/get POSIX signal timers with Expiration/TimerSetTimeFlags and report overruns, preserving libc timer identifiers and Drop cleanup.

#### Scenario: Nix POSIX signal timer lifecycle implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/sys/timer.rs` — `struct Timer`；`third_party/nix-ohos/src/sys/timer.rs` — `fn new`；`third_party/nix-ohos/src/sys/timer.rs` — `fn set`；`third_party/nix-ohos/src/sys/timer.rs` — `fn get`；`third_party/nix-ohos/src/sys/timer.rs` — `fn overruns`；`third_party/nix-ohos/src/sys/timer.rs` — `fn drop`。

### Requirement: Nix Linux timerfd lifecycle
TimerFd SHALL create timer file descriptors with ClockId/TimerFlags, set/get/unset Expiration values, wait/read expiration counts, expose raw descriptors, and close owned descriptors on drop.

#### Scenario: Nix Linux timerfd lifecycle implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/sys/timerfd.rs` — `struct TimerFd`；`third_party/nix-ohos/src/sys/timerfd.rs` — `fn as_raw_fd`；`third_party/nix-ohos/src/sys/timerfd.rs` — `fn from_raw_fd`；`third_party/nix-ohos/src/sys/timerfd.rs` — `enum ClockId`；`third_party/nix-ohos/src/sys/timerfd.rs` — `struct TimerFlags`；`third_party/nix-ohos/src/sys/timerfd.rs` — `fn new`；`third_party/nix-ohos/src/sys/timerfd.rs` — `fn set`；`third_party/nix-ohos/src/sys/timerfd.rs` — `fn get`；`third_party/nix-ohos/src/sys/timerfd.rs` — `fn unset`；`third_party/nix-ohos/src/sys/timerfd.rs` — `fn wait`；`third_party/nix-ohos/src/sys/timerfd.rs` — `fn drop`。

### Requirement: Nix process wait status
WaitStatus/WaitPidFlag/Id SHALL decode waitpid/wait/waitid results into exited/signaled/stopped/continued/syscall-stop states, expose status accessors, and preserve platform-specific id and raw-siginfo handling.

#### Scenario: Nix process wait status implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/sys/wait.rs` — `struct WaitPidFlag`；`third_party/nix-ohos/src/sys/wait.rs` — `enum WaitStatus`；`third_party/nix-ohos/src/sys/wait.rs` — `fn pid`；`third_party/nix-ohos/src/sys/wait.rs` — `fn exited`；`third_party/nix-ohos/src/sys/wait.rs` — `fn exit_status`；`third_party/nix-ohos/src/sys/wait.rs` — `fn signaled`；`third_party/nix-ohos/src/sys/wait.rs` — `fn term_signal`；`third_party/nix-ohos/src/sys/wait.rs` — `fn dumped_core`；`third_party/nix-ohos/src/sys/wait.rs` — `fn stopped`；`third_party/nix-ohos/src/sys/wait.rs` — `fn stop_signal`；`third_party/nix-ohos/src/sys/wait.rs` — `fn syscall_stop`；`third_party/nix-ohos/src/sys/wait.rs` — `fn stop_additional`；`third_party/nix-ohos/src/sys/wait.rs` — `fn continued`；`third_party/nix-ohos/src/sys/wait.rs` — `fn from_raw`；`third_party/nix-ohos/src/sys/wait.rs` — `fn decode_stopped`；`third_party/nix-ohos/src/sys/wait.rs` — `fn decode_stopped`；`third_party/nix-ohos/src/sys/wait.rs` — `fn from_siginfo`；`third_party/nix-ohos/src/sys/wait.rs` — `fn waitpid`；`third_party/nix-ohos/src/sys/wait.rs` — `fn wait`；`third_party/nix-ohos/src/sys/wait.rs` — `enum Id`；`third_party/nix-ohos/src/sys/wait.rs` — `fn waitid`。

### Requirement: Nix clock API
ClockId SHALL wrap clock identifiers, expose platform constants and pid CPU clock derivation, and wrap clock_getres/clock_gettime/clock_settime with typed Errno results.

#### Scenario: Nix clock API implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/time.rs` — `struct ClockId`；`third_party/nix-ohos/src/time.rs` — `fn from_raw`；`third_party/nix-ohos/src/time.rs` — `fn pid_cpu_clock_id`；`third_party/nix-ohos/src/time.rs` — `fn res`；`third_party/nix-ohos/src/time.rs` — `fn now`；`third_party/nix-ohos/src/time.rs` — `fn set_time`；`third_party/nix-ohos/src/time.rs` — `fn as_raw`；`third_party/nix-ohos/src/time.rs` — `const CLOCK_BOOTTIME`；`third_party/nix-ohos/src/time.rs` — `const CLOCK_BOOTTIME_ALARM`；`third_party/nix-ohos/src/time.rs` — `const CLOCK_MONOTONIC`；`third_party/nix-ohos/src/time.rs` — `const CLOCK_MONOTONIC_COARSE`；`third_party/nix-ohos/src/time.rs` — `const CLOCK_MONOTONIC_FAST`；`third_party/nix-ohos/src/time.rs` — `const CLOCK_MONOTONIC_PRECISE`；`third_party/nix-ohos/src/time.rs` — `const CLOCK_MONOTONIC_RAW`；`third_party/nix-ohos/src/time.rs` — `const CLOCK_PROCESS_CPUTIME_ID`；`third_party/nix-ohos/src/time.rs` — `const CLOCK_PROF`；`third_party/nix-ohos/src/time.rs` — `const CLOCK_REALTIME`；`third_party/nix-ohos/src/time.rs` — `const CLOCK_REALTIME_ALARM`；`third_party/nix-ohos/src/time.rs` — `const CLOCK_REALTIME_COARSE`；`third_party/nix-ohos/src/time.rs` — `const CLOCK_REALTIME_FAST`；`third_party/nix-ohos/src/time.rs` — `const CLOCK_REALTIME_PRECISE`；`third_party/nix-ohos/src/time.rs` — `const CLOCK_SECOND`；`third_party/nix-ohos/src/time.rs` — `const CLOCK_SGI_CYCLE`；`third_party/nix-ohos/src/time.rs` — `const CLOCK_TAI`；`third_party/nix-ohos/src/time.rs` — `const CLOCK_THREAD_CPUTIME_ID`；`third_party/nix-ohos/src/time.rs` — `const CLOCK_UPTIME`；`third_party/nix-ohos/src/time.rs` — `const CLOCK_UPTIME_FAST`；`third_party/nix-ohos/src/time.rs` — `const CLOCK_UPTIME_PRECISE`；`third_party/nix-ohos/src/time.rs` — `const CLOCK_VIRTUAL`；`third_party/nix-ohos/src/time.rs` — `fn from`；`third_party/nix-ohos/src/time.rs` — `fn from`；`third_party/nix-ohos/src/time.rs` — `fn fmt`；`third_party/nix-ohos/src/time.rs` — `fn clock_getres`；`third_party/nix-ohos/src/time.rs` — `fn clock_gettime`；`third_party/nix-ohos/src/time.rs` — `fn clock_settime`；`third_party/nix-ohos/src/time.rs` — `fn clock_getcpuclockid`。

### Requirement: Nix user context boundary
UContext SHALL wrap getcontext/setcontext and expose mutable/immutable signal-mask access where the target libc representation supports it, excluding musl and OHOS through cfg.

#### Scenario: Nix user context boundary implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/ucontext.rs` — `struct UContext`；`third_party/nix-ohos/src/ucontext.rs` — `fn get`；`third_party/nix-ohos/src/ucontext.rs` — `fn set`；`third_party/nix-ohos/src/ucontext.rs` — `fn sigmask_mut`；`third_party/nix-ohos/src/ucontext.rs` — `fn sigmask`。

### Requirement: Nix process and filesystem syscall boundary
unistd SHALL provide typed Uid/Gid/Pid/ForkResult/Whence/link/unlink flags and wrappers for process identity/groups, fork/exec/daemon/session/pgid, descriptor duplication and I/O, cwd/path/file creation, ownership, links, sync, IDs, and terminal/process helpers, converting libc sentinels through Errno and preserving documented unsafe boundaries.

#### Scenario: Nix process and filesystem syscall boundary implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/unistd.rs` — `struct Uid`；`third_party/nix-ohos/src/unistd.rs` — `fn from_raw`；`third_party/nix-ohos/src/unistd.rs` — `fn current`；`third_party/nix-ohos/src/unistd.rs` — `fn effective`；`third_party/nix-ohos/src/unistd.rs` — `fn is_root`；`third_party/nix-ohos/src/unistd.rs` — `fn as_raw`；`third_party/nix-ohos/src/unistd.rs` — `fn from`；`third_party/nix-ohos/src/unistd.rs` — `fn from`；`third_party/nix-ohos/src/unistd.rs` — `fn fmt`；`third_party/nix-ohos/src/unistd.rs` — `const ROOT`；`third_party/nix-ohos/src/unistd.rs` — `struct Gid`；`third_party/nix-ohos/src/unistd.rs` — `fn from_raw`；`third_party/nix-ohos/src/unistd.rs` — `fn current`；`third_party/nix-ohos/src/unistd.rs` — `fn effective`；`third_party/nix-ohos/src/unistd.rs` — `fn as_raw`；`third_party/nix-ohos/src/unistd.rs` — `fn from`；`third_party/nix-ohos/src/unistd.rs` — `fn from`；`third_party/nix-ohos/src/unistd.rs` — `fn fmt`；`third_party/nix-ohos/src/unistd.rs` — `struct Pid`；`third_party/nix-ohos/src/unistd.rs` — `fn from_raw`；`third_party/nix-ohos/src/unistd.rs` — `fn this`；`third_party/nix-ohos/src/unistd.rs` — `fn parent`；`third_party/nix-ohos/src/unistd.rs` — `fn as_raw`；`third_party/nix-ohos/src/unistd.rs` — `fn from`；`third_party/nix-ohos/src/unistd.rs` — `fn fmt`；`third_party/nix-ohos/src/unistd.rs` — `enum ForkResult`；`third_party/nix-ohos/src/unistd.rs` — `fn is_child`；`third_party/nix-ohos/src/unistd.rs` — `fn is_parent`；`third_party/nix-ohos/src/unistd.rs` — `fn fork`；`third_party/nix-ohos/src/unistd.rs` — `fn getpid`；`third_party/nix-ohos/src/unistd.rs` — `fn getppid`；`third_party/nix-ohos/src/unistd.rs` — `fn setpgid`；`third_party/nix-ohos/src/unistd.rs` — `fn getpgid`；`third_party/nix-ohos/src/unistd.rs` — `fn setsid`；`third_party/nix-ohos/src/unistd.rs` — `fn getsid`；`third_party/nix-ohos/src/unistd.rs` — `fn tcgetpgrp`；`third_party/nix-ohos/src/unistd.rs` — `fn tcsetpgrp`；`third_party/nix-ohos/src/unistd.rs` — `fn getpgrp`；`third_party/nix-ohos/src/unistd.rs` — `fn gettid`；`third_party/nix-ohos/src/unistd.rs` — `fn dup`；`third_party/nix-ohos/src/unistd.rs` — `fn dup2`；`third_party/nix-ohos/src/unistd.rs` — `fn dup3`；`third_party/nix-ohos/src/unistd.rs` — `fn dup3_polyfill`；`third_party/nix-ohos/src/unistd.rs` — `fn chdir`；`third_party/nix-ohos/src/unistd.rs` — `fn fchdir`；`third_party/nix-ohos/src/unistd.rs` — `fn mkdir`；`third_party/nix-ohos/src/unistd.rs` — `fn mkfifo`；`third_party/nix-ohos/src/unistd.rs` — `fn mkfifoat`；`third_party/nix-ohos/src/unistd.rs` — `fn symlinkat`；`third_party/nix-ohos/src/unistd.rs` — `fn reserve_double_buffer_size`；`third_party/nix-ohos/src/unistd.rs` — `fn getcwd`；`third_party/nix-ohos/src/unistd.rs` — `fn chown_raw_ids`；`third_party/nix-ohos/src/unistd.rs` — `fn chown`；`third_party/nix-ohos/src/unistd.rs` — `fn fchown`；`third_party/nix-ohos/src/unistd.rs` — `enum FchownatFlags`；`third_party/nix-ohos/src/unistd.rs` — `fn fchownat`；`third_party/nix-ohos/src/unistd.rs` — `fn to_exec_array`；`third_party/nix-ohos/src/unistd.rs` — `fn execv`；`third_party/nix-ohos/src/unistd.rs` — `fn execve`；`third_party/nix-ohos/src/unistd.rs` — `fn execvp`；`third_party/nix-ohos/src/unistd.rs` — `fn execvpe`；`third_party/nix-ohos/src/unistd.rs` — `fn fexecve`；`third_party/nix-ohos/src/unistd.rs` — `fn execveat`；`third_party/nix-ohos/src/unistd.rs` — `fn daemon`；`third_party/nix-ohos/src/unistd.rs` — `fn sethostname`；`third_party/nix-ohos/src/unistd.rs` — `type sethostname_len_t`；`third_party/nix-ohos/src/unistd.rs` — `type sethostname_len_t`；`third_party/nix-ohos/src/unistd.rs` — `fn gethostname`；`third_party/nix-ohos/src/unistd.rs` — `fn close`；`third_party/nix-ohos/src/unistd.rs` — `fn read`；`third_party/nix-ohos/src/unistd.rs` — `fn write`；`third_party/nix-ohos/src/unistd.rs` — `enum Whence`；`third_party/nix-ohos/src/unistd.rs` — `fn lseek`；`third_party/nix-ohos/src/unistd.rs` — `fn lseek64`；`third_party/nix-ohos/src/unistd.rs` — `fn pipe`；`third_party/nix-ohos/src/unistd.rs` — `fn pipe2`；`third_party/nix-ohos/src/unistd.rs` — `fn truncate`；`third_party/nix-ohos/src/unistd.rs` — `fn ftruncate`；`third_party/nix-ohos/src/unistd.rs` — `fn isatty`；`third_party/nix-ohos/src/unistd.rs` — `enum LinkatFlags`；`third_party/nix-ohos/src/unistd.rs` — `fn linkat`；`third_party/nix-ohos/src/unistd.rs` — `fn unlink`；`third_party/nix-ohos/src/unistd.rs` — `enum UnlinkatFlags`；`third_party/nix-ohos/src/unistd.rs` — `fn unlinkat`；`third_party/nix-ohos/src/unistd.rs` — `fn chroot`；`third_party/nix-ohos/src/unistd.rs` — `fn sync`；`third_party/nix-ohos/src/unistd.rs` — `fn syncfs`；`third_party/nix-ohos/src/unistd.rs` — `fn fsync`；`third_party/nix-ohos/src/unistd.rs` — `fn fdatasync`；`third_party/nix-ohos/src/unistd.rs` — `fn getuid`；`third_party/nix-ohos/src/unistd.rs` — `fn geteuid`；`third_party/nix-ohos/src/unistd.rs` — `fn getgid`；`third_party/nix-ohos/src/unistd.rs` — `fn getegid`；`third_party/nix-ohos/src/unistd.rs` — `fn seteuid`；`third_party/nix-ohos/src/unistd.rs` — `fn setegid`；`third_party/nix-ohos/src/unistd.rs` — `fn setuid`；`third_party/nix-ohos/src/unistd.rs` — `fn setgid`；`third_party/nix-ohos/src/unistd.rs` — `fn setfsuid`；`third_party/nix-ohos/src/unistd.rs` — `fn setfsgid`；`third_party/nix-ohos/src/unistd.rs` — `fn getgroups`；`third_party/nix-ohos/src/unistd.rs` — `fn setgroups`；`third_party/nix-ohos/src/unistd.rs` — `type setgroups_ngroups_t`；`third_party/nix-ohos/src/unistd.rs` — `type setgroups_ngroups_t`；`third_party/nix-ohos/src/unistd.rs` — `fn getgrouplist`；`third_party/nix-ohos/src/unistd.rs` — `type getgrouplist_group_t`；`third_party/nix-ohos/src/unistd.rs` — `type getgrouplist_group_t`；`third_party/nix-ohos/src/unistd.rs` — `fn initgroups`；`third_party/nix-ohos/src/unistd.rs` — `type initgroups_group_t`；`third_party/nix-ohos/src/unistd.rs` — `type initgroups_group_t`；`third_party/nix-ohos/src/unistd.rs` — `fn pause`；`third_party/nix-ohos/src/unistd.rs` — `fn set`；`third_party/nix-ohos/src/unistd.rs` — `fn cancel`；`third_party/nix-ohos/src/unistd.rs` — `fn alarm`；`third_party/nix-ohos/src/unistd.rs` — `fn sleep`；`third_party/nix-ohos/src/unistd.rs` — `fn enable`；`third_party/nix-ohos/src/unistd.rs` — `fn disable`；`third_party/nix-ohos/src/unistd.rs` — `fn mkstemp`；`third_party/nix-ohos/src/unistd.rs` — `enum PathconfVar`；`third_party/nix-ohos/src/unistd.rs` — `fn fpathconf`；`third_party/nix-ohos/src/unistd.rs` — `fn pathconf`；`third_party/nix-ohos/src/unistd.rs` — `enum SysconfVar`；`third_party/nix-ohos/src/unistd.rs` — `fn sysconf`；`third_party/nix-ohos/src/unistd.rs` — `fn pivot_root`；`third_party/nix-ohos/src/unistd.rs` — `fn setresuid`；`third_party/nix-ohos/src/unistd.rs` — `fn setresgid`；`third_party/nix-ohos/src/unistd.rs` — `struct ResUid`；`third_party/nix-ohos/src/unistd.rs` — `struct ResGid`；`third_party/nix-ohos/src/unistd.rs` — `fn getresuid`；`third_party/nix-ohos/src/unistd.rs` — `fn getresgid`；`third_party/nix-ohos/src/unistd.rs` — `struct AccessFlags`；`third_party/nix-ohos/src/unistd.rs` — `fn access`；`third_party/nix-ohos/src/unistd.rs` — `fn faccessat`；`third_party/nix-ohos/src/unistd.rs` — `fn eaccess`；`third_party/nix-ohos/src/unistd.rs` — `struct User`；`third_party/nix-ohos/src/unistd.rs` — `fn from`；`third_party/nix-ohos/src/unistd.rs` — `fn from`；`third_party/nix-ohos/src/unistd.rs` — `fn from_anything`；`third_party/nix-ohos/src/unistd.rs` — `fn from_uid`；`third_party/nix-ohos/src/unistd.rs` — `fn from_name`；`third_party/nix-ohos/src/unistd.rs` — `struct Group`；`third_party/nix-ohos/src/unistd.rs` — `fn from`；`third_party/nix-ohos/src/unistd.rs` — `fn members`；`third_party/nix-ohos/src/unistd.rs` — `fn from_anything`；`third_party/nix-ohos/src/unistd.rs` — `fn from_gid`；`third_party/nix-ohos/src/unistd.rs` — `fn from_name`；`third_party/nix-ohos/src/unistd.rs` — `fn ttyname`；`third_party/nix-ohos/src/unistd.rs` — `const PATH_MAX`；`third_party/nix-ohos/src/unistd.rs` — `fn getpeereid`；`third_party/nix-ohos/src/unistd.rs` — `fn chflags`。
