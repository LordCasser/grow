# shell-base 逐包核查

包路径：`crates/codegen/shell-base`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；已执行 cargo test --locked -p shell-base --all-features --lib，63 项通过、0 失败；未执行 Windows 目标。

## 模块与开关

- `crates/codegen/shell-base/Cargo.toml`
- `crates/codegen/shell-base/src/cpu_profile.rs`
- `crates/codegen/shell-base/src/env.rs`
- `crates/codegen/shell-base/src/lib.rs`
- `crates/codegen/shell-base/src/util/changelog.rs`
- `crates/codegen/shell-base/src/util/event_id.rs`
- `crates/codegen/shell-base/src/util/grow_home.rs`
- `crates/codegen/shell-base/src/util/mod.rs`
- `crates/codegen/shell-base/src/util/secure_file.rs`
- `crates/codegen/shell-base/src/util/tips.rs`
- `crates/codegen/shell-base/src/util/uname.rs`

Cargo feature：`{"test-support": [], "default-bazel": ["test-support"]}`。

## 功能与规范映射

- [CPU profiler lifecycle](../specs/developer-support/spec.md#requirement-cpu-profiler-lifecycle)：CpuProfileManager SHALL 从 Inactive 启动为 Active，take_stop_handle 转为 Stopping 并发布 watch=false，complete_stop 清除 Stopping 并发布 true；重复启动、无 active 停止、停止中操作返回对应 ControlErrorCode。
- [CPU profile artifact generation](../specs/developer-support/spec.md#requirement-cpu-profile-artifact-generation)：CPU profile SHALL 默认输出 grow_home/profiles 下带 PID 和 UTC 微秒时间的 folded 文件；显式 folded/txt 后缀按原路径，目录用 leader 前缀，其他路径用文件名作为前缀，最多尝试 32 个候选。
- [Shell test seams](../specs/developer-support/spec.md#requirement-shell-test-seams)：test-support feature SHALL 暴露 EnvVarGuard 和 CPU profiler 测试注入，default-bazel 启用 test-support；EnvVarGuard 持有共用锁直至 drop，恢复原 UTF-8 环境值或删除变量。
- [Process event identifier helpers](../specs/session-timeline/spec.md#requirement-process-event-identifier-helpers)：generate_event_id SHALL 将 session_id 与进程全局 SeqCst u64 fetch_add 序号连接；ensure_event_counter_at_least 用 fetch_max 只提高下次序号下界，供恢复方种入历史最大值后的起点。
- [Optional service endpoint predicates](../specs/http-credentials/spec.md#requirement-optional-service-endpoint-predicates)：is_cli_chat_proxy_url SHALL 接受与显式 GROW_CLI_CHAT_PROXY_BASE_URL 同 scheme/host/有效端口且路径相等或下一级斜杠前缀的 URL，另有 localhost/127.0.0.1/::1 host 字符串快捷判断。
- [Process liveness and termination helpers](../specs/application-maintenance/spec.md#requirement-process-liveness-and-termination-helpers)：进程辅助函数 SHALL 在 Unix 用 kill(pid,0) 检查，只有 ESRCH 为不存在，其他错误视为仍存在；Windows 打开 SYNCHRONIZE handle 并零等待，只有 WAIT_TIMEOUT 为仍运行。
- [Grow process name probes](../specs/application-maintenance/spec.md#requirement-grow-process-name-probes)：is_grow_process SHALL 在 Linux 读取 proc cmdline 并按 grow 子串匹配，Windows 查询完整 image path 并小写后子串匹配，其他平台运行 kill -0 只测存活。
- [Owner restricted file writes](../specs/http-credentials/spec.md#requirement-owner-restricted-file-writes)：write_secure_file SHALL 创建父目录、以 create/truncate 写入文件、flush 后调用权限收紧；Unix 新建 mode=0600，现有路径写后按权限检查收紧，Windows 写后设置仅当前用户的 protected DACL。
- [Local changelog cache and bullets](../specs/client-surfaces/spec.md#requirement-local-changelog-cache-and-bullets)：ChangelogManager::fetch SHALL 每次从实时非空 GROW_HOME 或 config grow_home fallback 定位 CHANGELOG.md/json，独立读取两种格式，不访问网络；空白/读取失败返回 None，JSON 整体解析失败返回 None 并记 debug。
- [Persistent tip rotation](../specs/client-surfaces/spec.md#requirement-persistent-tip-rotation)：pick_and_advance SHALL 对非空 tips 读取指定 home 的 tip_cursor.json，以 cursor 转 usize 后对列表长度取模选择并克隆条目，然后写 cursor+1；空列表返回 None 且不推进。
- [Shell utility formatting and reexports](../specs/developer-support/spec.md#requirement-shell-utility-formatting-and-reexports)：shell-base SHALL 提供 Unicode 字符截断、基于 RandomState 哈希的 53 位 [0,1) 采样数、rate 比较和 OS 内核版本辅助函数，并重新导出 config 路径与 client-support clipboard/stderr 能力。

## 边界

- 消耗 engine 并写产物，manager 状态仍由调用者 complete_stop 收尾；同步 stop 无论成功失败都执行 complete_stop。
- active 同步停止并返回 Some；其余返回 None，不等待其他 caller 持有的 stop handle；需要协调的 host 订阅 completion。
- Unix 支持，非 Unix 返回 unsupported；频率默认 1000 Hz，范围 1..4000，失败附带 provided/min/max。
- 开始返回 OutputPathCollision；真正创建在 stop 使用 create_new，再次拒绝覆盖，选路径并不提前预留文件。
- 按 thread 与反向 frames/symbols 形成 folded stack 计数行并排序，非空末尾加换行；屏蔽 libc/libgcc/pthread/vdso，报告/写入失败分类为 InternalError/ArtifactWriteFailed，不在本包渲染 flamegraph。
- 可独立覆盖平台能力/停止结果，注入启动仍校验频率和路径；锁只协调使用该 guard 的调用，不控制其他环境读写，非 UTF-8 原值按缺失处理。
- 生成 eventId，保留其他字段，仅在 agentTimestampMs 键不存在时加入当前毫秒时间。
- 直接返回，不检查格式也不补 timestamp；计数器不自行持久化，重启和溢出边界不能由进程内计数推导全局永久唯一。
- /v1 和 /v1/chat 匹配，/v11 不匹配；按实际路径拼接规则处理，配置末尾斜杠不被额外去除。
- 先要求 https 并拒绝 localhost 及 IPv4/IPv6 loopback，再使用上述端点规则；is_service_api_url 不添加该 https/loopback 限制。这些函数只判定，不发送或注入凭据。
- 默认 Unix SIGTERM，可选 SIGKILL，ESRCH 成功；Windows 两种信号都 TerminateProcess，OpenProcess 的 ERROR_INVALID_PARAMETER 视为已结束，其他错误透传。
- 本层不持有进程身份句柄跨检查与终止，Unix 将 u32 转 i32，不自动过滤 0 或转换后的负 PID；调用方负责有效 PID 与目标所有权。
- macOS/BSD 使用 ps comm 首行 basename 小写后匹配 grow 子串，Linux/Windows 委托普通探测；失败返回 false，不提供可执行文件精确身份或 PID 重用防护保证。
- 只负责打开和新建权限，现有路径不会因 mode 自动收紧；无原子 rename、fsync 或 symlink 拒绝保证，不将其描述为加密存储。
- NotFound 忽略，其他错误返回；Unix 低九位已经 0600 时跳过 chmod，否则设 0600，其他非 Unix/Windows 平台无操作。
- 缺失 category/description/breaking_change 使用默认值，类型错误仍可使整个数组失败。
- 过滤原 description 空字符串，取最多 max 项并去除双星号和反引号；不 trim 空白、不保证去格式后仍非空，保留原顺序。
- 读取回退 0，保存错误静默忽略，本函数不创建 home、不加锁或原子替换；每次调用而非强制每个 session 推进，列表来源由 caller 提供，不在此请求远程设置。
- 截断按字符边界不加省略号；采样比较 random<rate，无额外 clamp，非密码学保证，NaN 比较为 false。
- Unix 返回小写 kernel 加 release，Windows 提取 [Version ...] 为 windows 版本；失败回退 std::env::consts::OS。本包重导出不另实现 home、clipboard 或 stderr 锁。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。

## 验证边界

10 个 Rust 文件与 manifest 已完整读取；CPU profile 生命周期测试使用 fake engine，不能据此宣称真实 pprof 栈采集已验证。Unix 和 Windows 分支均有源码证据，但本机只执行 macOS 构建。进程探测是 best effort；相关身份/PID 边界已独立登记 backlog。CPU profile 测试与 EnvVarGuard 的 test-support/default-bazel feature 均已枚举。
