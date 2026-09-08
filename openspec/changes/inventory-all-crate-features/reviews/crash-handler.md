# crash-handler 逐包核查

包路径：`crates/codegen/crash-handler`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；已执行 cargo test --locked -p crash-handler，15 项单元、7 项集成、1 项 doctest 通过；1 个子进程入口标记 ignored 并由集成测试显式调用。

## 模块与开关

- `crates/codegen/crash-handler/Cargo.toml`
- `crates/codegen/crash-handler/src/format.rs`
- `crates/codegen/crash-handler/src/handler.rs`
- `crates/codegen/crash-handler/src/lib.rs`
- `crates/codegen/crash-handler/src/symbolicate.rs`
- `crates/codegen/crash-handler/src/terminal.rs`
- `crates/codegen/crash-handler/tests/integration.rs`

Cargo feature：`{}`。

## 功能与规范映射

- [Crash installation and startup ordering](../specs/crash-reporting/spec.md#requirement-crash-installation-and-startup-ordering)：crash-handler SHALL 提供 install、check_previous_crash 与 terminal restore 控制；caller 必须先消费旧 blob，再在 terminal/线程/runtime 初始化前安装，因为 install 会截断 last-crash.bin。
- [Unix fatal signal capture](../specs/crash-reporting/spec.md#requirement-unix-fatal-signal-capture)：Unix handler SHALL 注册 SIGBUS/SIGSEGV/SIGABRT，使用 SA_SIGINFO/SA_ONSTACK/SA_RESETHAND；full handler 先 alarm(3)，写 blob，再恢复 termios 并设置默认 disposition、重发原信号。
- [Best effort crash frames and raw writes](../specs/crash-reporting/spec.md#requirement-best-effort-crash-frames-and-raw-writes)：full handler SHALL 使用固定最大 64 frame 缓冲，先写含 crash PC 的 blob，再 best effort 读取 frame-pointer chain 并从文件起点覆盖较完整 blob，避免 walk 失败前毫无记录。
- [Windows fatal exception filter](../specs/crash-reporting/spec.md#requirement-windows-fatal-exception-filter)：Windows SHALL 使用 SetUnhandledExceptionFilter 处理 access violation、stack overflow、in-page error、illegal instruction、array bounds exceeded，其他异常继续搜索；不通过此 filter 捕获 abort()。
- [Terminal restoration mode switching](../specs/crash-reporting/spec.md#requirement-terminal-restoration-mode-switching)：install_terminal_restore_only SHALL 安装不写 blob 的基本 handler；enable/disable_terminal_escape_restore 根据当前 fd/handle 是否有效选择完整或 minimal handler，控制是否发送转义序列。
- [GCRX binary crash format](../specs/crash-reporting/spec.md#requirement-gcrx-binary-crash-format)：CrashBlob SHALL 使用 GCRX magic、格式版本 1、64 字节 header 和最多 64 个 little-endian u64 frame；header 包含 signal u8、si_code i32、address u64、pid u32、timestamp u64、frame count u16 及 32 字节 NUL padded app_version。
- [Previous crash report consumption and history](../specs/crash-reporting/spec.md#requirement-previous-crash-report-consumption-and-history)：check_previous_crash SHALL 读取 last-crash.bin，解析成功后生成 last-crash-report.txt 并写 history/crash-<timestamp>.txt，按文件名字典序裁剪所有 txt 到最多 5 个，再尝试删除 blob，返回 CrashReport。
- [Crash symbolication and report text](../specs/crash-reporting/spec.md#requirement-crash-symbolication-and-report-text)：resolve_frames SHALL 在正常启动上下文将记录的原始 IP 交给当前进程 backtrace::resolve，保留 IP、可选 symbol/file/line；format_report 输出信号、code 说明、地址、PID、版本、Unix 秒和逐帧文本。

## 边界

- 创建目录并预开 crash 文件，保存最多 32 字节版本；Unix 设置并收紧 0600，Windows CREATE_ALWAYS 且无共享，不额外建立 owner-only ACL；不支持的平台返回 false。
- 继续保存 stdin termios、设置 altstack 和信号处理后返回 true，但 sigaction/sigaltstack 返回值未检查；重复安装未关闭旧全局 fd/handle，也不支持并发安装保证。
- 提取 crash PC 和 frame pointer；其他架构或空 context 返回 0，非 Linux/macOS 的 fault address 为 0。
- 保存 fd0 termios，最多尝试一次 mmap 16 KiB + sigaltstack；标记在分配前设置，失败不重试。altstack 仅适用于安装线程，不覆盖其他 worker 栈溢出。
- 停止 walk；这不验证映射可读性，raw pointer 解引用仍可触发次生 fault。
- 返回值未用于补写/持久化确认，没有 fsync 或并发重入串行保证；不能承诺总能保存完整原始 crash，次生信号可能改变退出状态。
- exception code 写 si_code，access violation 至少两个参数时记录 fault address；FILETIME 转 Unix 秒，in-page/illegal 分别映射 signal 7/4，其余映射 11，最后 CONTINUE_SEARCH。
- x86_64 取 Rip/Rbp，其他架构实际 PC/FP 都为零，不能按注释称 ARM64 已捕获 PC；full 带 terminal 的 wrapper 在内层返回后仍执行转义恢复。
- 默认不发 escape，Unix 仅恢复保存的 termios；Windows basic minimal 只 CONTINUE_SEARCH；需要 caller 在 TUI 启用时显式 enable。
- 先结束同步更新、显示 cursor，关闭 1000/1002/1003/1015/1006 mouse、2004 paste、1004 focus，再 pop kitty protocol，最后退出 alt screen；另提供 mouse-only 与 mouse+paste reset。Unix 对 stderr raw write，Windows WriteFile，写失败不向 caller 报告。
- 返回 None；额外尾字节不拒绝，未知 signal 不拒绝，u64 frame cast 为 usize。
- parse 将整个 version 字段变为空串；正常字段只剥离末尾 NUL，不做控制字符清洗。writer 的 unsafe API 依赖 caller 提供足够缓冲。
- 错误被忽略，仍尝试删除 blob 并返回 report_path；不保证返回的报告文件存在，不是事务性消费。无效/缺失 blob 返回 None，且此函数不删除无效数据。
- Unix 新建 0600、既有文件在写内容前 fchmod 0600；非 Unix std::fs::write，pathname 跟随 symlink。相同 timestamp 可覆盖，裁剪不是按 mtime，未限定 crash- 前缀。
- blob 不含模块标识/load base/相对地址，不做 ASLR 重定位或版本匹配；可能 unknown 或错误解析，不能保证下一次启动得到准确源码位置。
- symbol 显示 <unknown>，仅 file 和 line 同时有值才打印位置；时间实际为 Unix 数字而非 ISO 8601，SIGILL/ABRT/BUS/SEGV 有名称，其余 Unknown signal。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。

## 验证与文档差异

已读全部 5 个 src 模块、tests/integration.rs、manifest 和 README。macOS 测试使用 re-exec 自有子进程触发 SIGBUS/SIGSEGV/abort 并检查 blob，正常 Tokio/I/O/信号共存用例通过；日志 `/tmp/grow-crash-handler-tests.log`。未验证 Windows SEH、其他架构、worker stack overflow、I/O 故障及并发信号。

README 的 non-Unix no-op、默认恢复 escape、始终捕获 PC 与代码不一致：存在 Windows filter，默认 basic handler 无 escape，非支持架构 PC=0。Windows ARM64 注释称捕获 PC，实际非 x86_64 分支全零。按实现写入 delta，不据旧说明扩大保证。

符号解析直接使用旧绝对 IP，不具有跨进程 ASLR/binary 身份验证。报告写失败仍删 blob 的分支可见于代码，未注入故障；历史保留按文件名排序且相同 timestamp 可覆盖。有关完整性和生命周期限制登记 backlog，未修改实现。
