# tty-utils 逐包核查

包路径：`crates/codegen/tty-utils`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；已执行 cargo test --locked -p tty-utils：29 项主测试和 2 项编译 doctest 通过；stderr 测试另启动一次子进程执行 1 项，包含在主测试的验证路径中，不重复累计。

## 模块与开关

- `crates/codegen/tty-utils/Cargo.toml`
- `crates/codegen/tty-utils/src/lib.rs`
- `crates/codegen/tty-utils/src/process_scope.rs`
- `crates/codegen/tty-utils/src/runtime.rs`

Cargo feature：`{}`。

## 功能与规范映射

- [Terminal detach command hooks](../specs/process-lifecycle/spec.md#requirement-terminal-detach-command-hooks)：detach_command 和 detach_std_command SHALL 在 Unix pre_exec 调用 setsid，EPERM 时回退 setpgid(0,0)，其他失败返回 errno；Windows 设置 CREATE_NO_WINDOW。
- [Linux parent death binding](../specs/process-lifecycle/spec.md#requirement-linux-parent-death-binding)：kill_on_parent_death_std SHALL 仅在 Linux 注册 pre_exec，设置 PR_SET_PDEATHSIG(SIGTERM)，并核对当前 ppid 与 arm 时捕获的父 PID，不一致立即 _exit(0)；其他平台无操作。
- [Validated process group teardown](../specs/process-lifecycle/spec.md#requirement-validated-process-group-teardown)：Unix ProcessGroupId SHALL 拒绝 0、1、大于 i32::MAX 及调用者自身 pgid；ProcessGroup attach/attach_std/attach_pid 保存校验后的 group leader，terminate/kill 分别 killpg SIGTERM/SIGKILL。
- [Windows job enrollment and resume](../specs/process-lifecycle/spec.md#requirement-windows-job-enrollment-and-resume)：Windows ProcessGroup SHALL 新建设置 KILL_ON_JOB_CLOSE 的 Job Object，以 PROCESS_SET_QUOTA|PROCESS_TERMINATE 打开目标后分配入 job，关闭临时 handle；terminate/kill 都以退出码 1 终止 job，Drop 关闭 job。
- [Weak owned process scope](../specs/process-lifecycle/spec.md#requirement-weak-owned-process-scope)：ProcessScope SHALL clone 共享 scope，注册 Weak<ProcessGroup> 而由 caller 持有强 Arc；prepare 设置新 group，enroll attach 后注册，spawn 组合 prepare/spawn/enroll。
- [Scope hangup grace and cleanup](../specs/process-lifecycle/spec.md#requirement-scope-hangup-grace-and-cleanup)：kill_all 和最后一个 ScopeInner Drop SHALL 对仍可升级的 Weak 执行清理：先 hangup 标记的 terminal group，有任何标记则共享等待 200ms，再重新升级 Weak 并 kill；信号错误 best effort 忽略。
- [Noninteractive git command environment](../specs/process-lifecycle/spec.md#requirement-noninteractive-git-command-environment)：pager_env SHALL 将 PAGER/GIT_PAGER/GH_PAGER/MANPAGER/SYSTEMD_PAGER 设为 Unix cat 或其他平台空串，AWS_PAGER/GPG_TTY 为空，Git editor 为 Unix true 或 Windows cmd exit，GIT_TERMINAL_PROMPT=0。
- [Native stderr redirection and duplication](../specs/process-lifecycle/spec.md#requirement-native-stderr-redirection-and-duplication)：redirect_native_stderr SHALL 在 Unix best effort 复制 fd2 到进程期 OnceLock，flush Rust stderr 后将 fd2 指向 devnull；dup_tui_stderr 返回独立拥有的原 stderr 副本，未重定向时复制当前 fd2。
- [Cached WSL detection](../specs/process-lifecycle/spec.md#requirement-cached-wsl-detection)：is_wsl SHALL 缓存进程生命周期判定，非 Linux 为 false；Linux 通过 WSL_DISTRO_NAME/WSL_INTEROP 键存在或 osrelease 的不区分大小写 microsoft/wsl 子串判定。
- [Tokio worker budget](../specs/process-lifecycle/spec.md#requirement-tokio-worker-budget)：capped_worker_threads SHALL 读取 available_parallelism，失败回退 1，并将返回值上限设为 8；cap_worker_threads 为 NonZeroUsize 纯函数。

## 边界

- 只有进程组隔离，仍可能访问 controlling TTY；辅助函数不自动重定向 stdin/stdout/stderr。
- 各 helper 设置自己的 flag 值，不保证自动合并；新进程组 helper 单独设置 CREATE_NEW_PROCESS_GROUP。
- 返回 EINVAL；实际绑定的是 spawning thread 生命周期，release 不强制线程检查，setuid/setcap exec 可清除设置。
- Linux 设置相同信号并返回 prctl 错误，不检查 arm 前父进程已死亡的竞态，需 caller EOF 处理；其他平台 Ok 无操作。
- 未 attach 为 Ok 无操作；attach 不核实目标实际 pgid 或持有身份句柄，caller 必须保证 group leader 条件并在 reap 时释放 handle。
- 先 SIGHUP 再 SIGCONT，便于停止的 shell 转发信号；Unix Drop 不杀进程，信号 errno 向直接 caller 传播，跨组 job 不由 killpg 直接覆盖。
- 启动 flags 包含 NEW_PROCESS_GROUP/NO_WINDOW/SUSPENDED；resume_enrolled 枚举目标 PID 首个线程并 ResumeThread，找不到或 API 失败返回错误，调用者须保证先 attach 后 resume，本方法不独立校验所属 job。
- 无操作；Windows hangup 无操作且 wants_hangup 为 false。
- live_count 不再计入，kill_all 跳过该 Weak；该安全边界依赖 caller 在正确 reap 时释放，live_count 不探测操作系统进程。
- 普通 group 当场 best effort kill 并返回 false；已要求 hangup 的 bare register 不杀，须通过 enroll_terminal_pid 或 caller 自行负责。
- 错误上抛；本函数没有显式 child wait 或通用回滚，不能推导所有 enrollment 错误都已回收。
- 第二次升级失败则跳过，避免主动延长 group handle 生命周期；本层不 wait 子进程，owner 负责 reap。
- 晚到 terminal 走同样 hangup/grace/kill 并返回错误；global scope 是 OnceLock 单例，关闭后不重开，is_closed 只反映关闭 latch。
- 应用 detach、stdin null、pager 环境及 GIT_ASKPASS 空串/GIT_LFS_SKIP_SMUDGE=1/GIT_SSH_COMMAND BatchMode，并添加 --no-optional-locks；不在构造时执行命令。
- 相对路径以当前目录解析，文件名恰为 git 时设置 GIT_EXEC_PATH 为父目录，其他 wrapper 保留自身 exec path；缺失使用 git。
- Unix dup2 恢复原 stderr；redirect/restore 不返回失败，需在启动线程前由 caller 安排。
- redirect/restore 无操作，dup 从 GetStdHandle 通过 try_clone 获得独立 handle，不关闭原标准错误 handle；不保证控制台 code page 的 Unicode 显示。
- 缓存不刷新；纯输入辅助检查按完整环境键匹配，值为空也视为存在，读取不到 osrelease 且无键则 false。
- 返回 8；输入 1..8 原样返回，本包只返回预算，不自动创建或修改运行时。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。

## 平台和所有权边界

3 个 Rust 文件及 manifest 全部读取。MacOS 测试覆盖进程组/孙进程终止、scope 关闭竞态、stderr 子进程隔离与纯 WSL 输入判定。Linux pdeathsig 和 Windows Job Object/suspend/resume 只完成源码核查，本次未在目标平台执行。ProcessScope 不 wait/reap；安全性依赖 owner 持有并在适当时机释放 Arc。spawn 后 enrollment 失败没有统一回滚分支，已登记后续审计，不在本文档变更内修改。
