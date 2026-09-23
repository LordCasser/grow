## ADDED Requirements

### Requirement: Crash installation and startup ordering
crash-handler SHALL 提供 install、check_previous_crash 与 terminal restore 控制；caller 必须先消费旧 blob，再在 terminal/线程/runtime 初始化前安装，因为 install 会截断 last-crash.bin。

#### Scenario: 平台安装
- **WHEN** Unix 或 Windows 安装
- **THEN** 创建目录并预开 crash 文件，保存最多 32 字节版本；Unix 设置并收紧 0600，Windows CREATE_ALWAYS 且无共享，不额外建立 owner-only ACL；不支持的平台返回 false。

#### Scenario: 安装结果边界
- **WHEN** Unix 文件打开和 fchmod 成功
- **THEN** 继续保存 stdin termios、设置 altstack 和信号处理后返回 true，但 sigaction/sigaltstack 返回值未检查；重复安装未关闭旧全局 fd/handle，也不支持并发安装保证。

证据：`crates/codegen/crash-handler/src/lib.rs` — `install`；`crates/codegen/crash-handler/src/handler.rs` — `install`；`crates/codegen/crash-handler/src/handler.rs` — `CRASH_FD`。

### Requirement: Unix fatal signal capture
Unix handler SHALL 注册 SIGBUS/SIGSEGV/SIGABRT，使用 SA_SIGINFO/SA_ONSTACK/SA_RESETHAND；full handler 先 alarm(3)，写 blob，再恢复 termios 并设置默认 disposition、重发原信号。

#### Scenario: 上下文
- **WHEN** Linux/macOS 的 x86_64/aarch64 提供非空 context
- **THEN** 提取 crash PC 和 frame pointer；其他架构或空 context 返回 0，非 Linux/macOS 的 fault address 为 0。

#### Scenario: 终端备用栈
- **WHEN** 调用 minimal 或 full install
- **THEN** 保存 fd0 termios，最多尝试一次 mmap 16 KiB + sigaltstack；标记在分配前设置，失败不重试。altstack 仅适用于安装线程，不覆盖其他 worker 栈溢出。

证据：`crates/codegen/crash-handler/src/handler.rs` — `extract_pc_and_fp`；`crates/codegen/crash-handler/src/handler.rs` — `register_crash_signals`；`crates/codegen/crash-handler/src/handler.rs` — `setup_alt_stack`；`crates/codegen/crash-handler/src/handler.rs` — `restore_termios_and_reraise`。

### Requirement: Best effort crash frames and raw writes
full handler SHALL 使用固定最大 64 frame 缓冲，先写含 crash PC 的 blob，再 best effort 读取 frame-pointer chain 并从文件起点覆盖较完整 blob，避免 walk 失败前毫无记录。

#### Scenario: 链终止
- **WHEN** frame pointer 为 0、小于 4096、未对齐、返回地址小于 4096 或前链不向上
- **THEN** 停止 walk；这不验证映射可读性，raw pointer 解引用仍可触发次生 fault。

#### Scenario: I/O 与并发
- **WHEN** raw write/seek 部分失败或多个 fatal signal 并发
- **THEN** 返回值未用于补写/持久化确认，没有 fsync 或并发重入串行保证；不能承诺总能保存完整原始 crash，次生信号可能改变退出状态。

证据：`crates/codegen/crash-handler/src/handler.rs` — `walk_frame_pointers`；`crates/codegen/crash-handler/src/handler.rs` — `write_crash_blob`；`crates/codegen/crash-handler/src/handler.rs` — `write_to_handle`。

### Requirement: Windows fatal exception filter
Windows SHALL 使用 SetUnhandledExceptionFilter 处理 access violation、stack overflow、in-page error、illegal instruction、array bounds exceeded，其他异常继续搜索；不通过此 filter 捕获 abort()。

#### Scenario: 记录字段
- **WHEN** 满足 fatal exception 且句柄有效
- **THEN** exception code 写 si_code，access violation 至少两个参数时记录 fault address；FILETIME 转 Unix 秒，in-page/illegal 分别映射 signal 7/4，其余映射 11，最后 CONTINUE_SEARCH。

#### Scenario: 架构和转义
- **WHEN** Windows x86_64 或其他架构运行
- **THEN** x86_64 取 Rip/Rbp，其他架构实际 PC/FP 都为零，不能按注释称 ARM64 已捕获 PC；full 带 terminal 的 wrapper 在内层返回后仍执行转义恢复。

证据：`crates/codegen/crash-handler/src/handler.rs` — `is_fatal_exception`；`crates/codegen/crash-handler/src/handler.rs` — `exception_to_signal`；`crates/codegen/crash-handler/src/handler.rs` — `crash_handler_with_terminal`。

### Requirement: Terminal restoration mode switching
install_terminal_restore_only SHALL 安装不写 blob 的基本 handler；enable/disable_terminal_escape_restore 根据当前 fd/handle 是否有效选择完整或 minimal handler，控制是否发送转义序列。

#### Scenario: 默认状态
- **WHEN** 刚执行 minimal 或 full install
- **THEN** 默认不发 escape，Unix 仅恢复保存的 termios；Windows basic minimal 只 CONTINUE_SEARCH；需要 caller 在 TUI 启用时显式 enable。

#### Scenario: 恢复序列
- **WHEN** 发出 RESTORE_SEQ
- **THEN** 先结束同步更新、显示 cursor，关闭 1000/1002/1003/1015/1006 mouse、2004 paste、1004 focus，再 pop kitty protocol，最后退出 alt screen；另提供 mouse-only 与 mouse+paste reset。Unix 对 stderr raw write，Windows WriteFile，写失败不向 caller 报告。

证据：`crates/codegen/crash-handler/src/handler.rs` — `install_terminal_restore_only`；`crates/codegen/crash-handler/src/handler.rs` — `enable_terminal_escape_restore`；`crates/codegen/crash-handler/src/handler.rs` — `disable_terminal_escape_restore`；`crates/codegen/crash-handler/src/terminal.rs` — `RESTORE_SEQ`；`crates/codegen/crash-handler/src/terminal.rs` — `MOUSE_TRACKING_RESET`。

### Requirement: GCRX binary crash format
CrashBlob SHALL 使用 GCRX magic、格式版本 1、64 字节 header 和最多 64 个 little-endian u64 frame；header 包含 signal u8、si_code i32、address u64、pid u32、timestamp u64、frame count u16 及 32 字节 NUL padded app_version。

#### Scenario: 解析拒绝
- **WHEN** magic/version 错、header 或 frame 数据不足、frame count 超过 64
- **THEN** 返回 None；额外尾字节不拒绝，未知 signal 不拒绝，u64 frame cast 为 usize。

#### Scenario: 版本 UTF-8
- **WHEN** 版本按 32 字节截断导致非法 UTF-8
- **THEN** parse 将整个 version 字段变为空串；正常字段只剥离末尾 NUL，不做控制字符清洗。writer 的 unsafe API 依赖 caller 提供足够缓冲。

证据：`crates/codegen/crash-handler/src/format.rs` — `CrashBlob`；`crates/codegen/crash-handler/src/format.rs` — `HEADER_SIZE`；`crates/codegen/crash-handler/src/format.rs` — `write_header`；`crates/codegen/crash-handler/src/format.rs` — `write_frame`。

### Requirement: Previous crash report consumption and history
check_previous_crash SHALL 读取 last-crash.bin，解析成功后生成 last-crash-report.txt 并写 history/crash-<timestamp>.txt，按文件名字典序裁剪所有 txt 到最多 5 个，再尝试删除 blob，返回 CrashReport。

#### Scenario: 失败边界
- **WHEN** 报告或历史写入失败但 blob 有效
- **THEN** 错误被忽略，仍尝试删除 blob 并返回 report_path；不保证返回的报告文件存在，不是事务性消费。无效/缺失 blob 返回 None，且此函数不删除无效数据。

#### Scenario: 权限和保留
- **WHEN** 写人类可读报告或重复 timestamp
- **THEN** Unix 新建 0600、既有文件在写内容前 fchmod 0600；非 Unix std::fs::write，pathname 跟随 symlink。相同 timestamp 可覆盖，裁剪不是按 mtime，未限定 crash- 前缀。

证据：`crates/codegen/crash-handler/src/lib.rs` — `check_previous_crash`；`crates/codegen/crash-handler/src/lib.rs` — `archive_report`；`crates/codegen/crash-handler/src/lib.rs` — `write_owner_only`。

### Requirement: Crash symbolication and report text
resolve_frames SHALL 在正常启动上下文将记录的原始 IP 交给当前进程 backtrace::resolve，保留 IP、可选 symbol/file/line；format_report 输出信号、code 说明、地址、PID、版本、Unix 秒和逐帧文本。

#### Scenario: 解析保证
- **WHEN** 旧进程地址布局或 binary 已改变
- **THEN** blob 不含模块标识/load base/相对地址，不做 ASLR 重定位或版本匹配；可能 unknown 或错误解析，不能保证下一次启动得到准确源码位置。

#### Scenario: 格式细节
- **WHEN** frame 无 symbol 或缺少 file/line
- **THEN** symbol 显示 <unknown>，仅 file 和 line 同时有值才打印位置；时间实际为 Unix 数字而非 ISO 8601，SIGILL/ABRT/BUS/SEGV 有名称，其余 Unknown signal。

证据：`crates/codegen/crash-handler/src/symbolicate.rs` — `resolve_frames`；`crates/codegen/crash-handler/src/symbolicate.rs` — `format_report`；`crates/codegen/crash-handler/src/symbolicate.rs` — `signal_name`。
