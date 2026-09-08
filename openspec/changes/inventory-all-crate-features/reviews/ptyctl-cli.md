# ptyctl-cli 逐包核查

包路径：`crates/codegen/ptyctl-cli`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；本包没有内置测试；已执行默认构建和显式依赖 feature 构建，并用后者做 8 项 mock HTTP/CLI 检查，结果和限制见下文。

## 模块与开关

- `crates/codegen/ptyctl-cli/Cargo.toml`
- `crates/codegen/ptyctl-cli/src/cli.rs`
- `crates/codegen/ptyctl-cli/src/commands/client.rs`
- `crates/codegen/ptyctl-cli/src/commands/mod.rs`
- `crates/codegen/ptyctl-cli/src/commands/run.rs`
- `crates/codegen/ptyctl-cli/src/main.rs`
- `crates/codegen/ptyctl-cli/src/registry.rs`

Cargo feature：`{}`。

## 功能与规范映射

- [PTY CLI command routing](../specs/pty-control/spec.md#requirement-pty-cli-command-routing)：ptyctl SHALL 提供 run/send/screen/status/stop/resize/cursor/wait/list 子命令；网络客户端启动前安装 TLS ring provider，CLI 通过 clap 检查必需参数和冲突。
- [PTY CLI launch configuration](../specs/pty-control/spec.md#requirement-pty-cli-launch-configuration)：run SHALL 默认终端 80x24、loopback 端口 0 自动分配，可指定 cwd、重复 env、name、force、timeout、linger 和 quiet；将 command 原参数数组传给 PtySession。
- [Named PTY registry persistence](../specs/pty-control/spec.md#requirement-named-pty-registry-persistence)：会话登记 SHALL 采用 PTYCTL_SESSION_DIR 优先，否则使用系统 state_dir 或 data_local_dir 下 ptyctl/sessions；保存 port、可选 child pid、command、cwd 和 UTC started_at。
- [Named PTY takeover and reachability](../specs/pty-control/spec.md#requirement-named-pty-takeover-and-reachability)：未启用 force 时 run SHALL 拒绝 lookup 成功且 server_alive 的同名登记；失效/不可解析登记可替换，force 仅替换登记，不停止旧 server。
- [PTY CLI keystrokes resize and stop](../specs/pty-control/spec.md#requirement-pty-cli-keystrokes-resize-and-stop)：send SHALL POST /control/send 的 keys，enter 选项追加 <CR>；resize 按小写 x 分割并解析两个 u16，再 POST /control/resize；stop POST /control/stop。
- [PTY CLI screen formats and projection](../specs/pty-control/spec.md#requirement-pty-cli-screen-formats-and-projection)：screen SHALL 传递可选 rows/cols/cursor、format 和 full 查询参数；styled/html 打印原响应正文，否则从 JSON lines 数组打印逐行文本。
- [PTY CLI status and cursor passthrough](../specs/pty-control/spec.md#requirement-pty-cli-status-and-cursor-passthrough)：status 和 cursor SHALL 分别 GET /query/status 与 /query/cursor 并打印响应文本，网络/读取错误上抛。
- [PTY CLI condition wait outcomes](../specs/pty-control/spec.md#requirement-pty-cli-condition-wait-outcomes)：wait SHALL 由 clap 要求 text/regex/gone/stable_ms 中一个条件，默认 timeout 10 秒；发起 /wait 请求，timeout_ms 使用饱和乘 1000，HTTP timeout 使用饱和加 5 秒。

## 边界

- 必须且只能选择 host、port、name 之一；host 以 http 开头时原样使用，否则添加 http://，port 和登记 name 解析为 loopback URL；不按登记 PID 连接。
- 展示帮助并由 clap 返回用法错误。
- 按第一个等号分割，重复 key 后值覆盖前值，无等号项忽略；本层不额外校验空 key、零尺寸或零 timeout。
- 随后绑定 127.0.0.1，登记实际端口；quiet 仅向 stdout 打印端口，否则向 stderr 打印 command/PID/port。Session 启动先于 bind，不能推导端口失败前没有启动 child。
- 以 name.json 存储，先写固定 .name.json.tmp 再 rename；不存在或 JSON 无效时报错。name 直接拼入路径，本层未校验路径分隔符或提供并发 CAS。
- 存在时删除；枚举只接受非点开头的 json 且可解析 SessionInfo，跳过坏文件，不保证排序，也不自动清理 dead 登记。
- 500ms HTTP client 请求 /query/status，只要求状态恰为 200 且 JSON 存在 size 字段；这不证明进程身份，也不要求 child alive。
- best effort 注销；已指向其他端口则保留。本层没有给 serve 绑定 Session 退出的 graceful shutdown，不能由 timeout/linger 参数推导 HTTP server 会退出。
- 逐项探测并展示 server 状态；JSON 键为 server_alive，区别于 child alive，空文本列表显示 No active sessions。
- 读取响应正文并返回错误；成功 resize 和 stop 分别打印结果，send 不打印成功正文。
- 在发送请求前报错；本层允许零数值，合法尺寸的进一步限制属于服务端。
- clap 接受并与其他格式互斥，但 main 忽略这两个字段，实际仍请求 text 并打印 lines；不得将帮助文字当成已实现 JSON/ANSI 输出。
- 行号从本次输出的 1 开始，非字符串项为空串，无 lines 则无输出；styled/html 不加行号。非成功 HTTP 状态报错。
- 这两个命令没有 error_for_status 或状态检查，仍打印正文并正常返回；不保证 CLI 成功退出表示 HTTP 查询成功。
- 完整 pretty JSON 输出到 stdout；matched 为 true 则退出 0，false 或缺失/非布尔则退出 1。
- stderr 打印 Error 并退出 2；等待条件实现及 120 秒服务端上限继续在 ptyctl 库核对，CLI 本身未 clamp 到 120。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。

## 构建与行为验证

`cargo build --locked -p ptyctl-cli` 退出 101：六处 RequestBuilder.query 不存在。根 reqwest 关闭默认 feature，未启用 query；锁定 0.13.4 源码的 query 方法受该 feature 控制。未修改 manifest 或源码。

`cargo build --locked -p ptyctl-cli --features reqwest/query` 退出 0。对此明确附加 feature 的 binary，临时 loopback mock HTTP 检查 8 项通过：json/ansi 参数均仍请求 text 并打印普通行；status/cursor 收到 HTTP 500 仍退出 0；wait matched/miss 分别退出 0/1；重复等待条件和冲突 target 退出 2。mock server 已关闭，未连接已有用户会话。日志见 `/tmp/grow-ptyctl-cli-build.log`、`/tmp/grow-ptyctl-cli-query-build.log`、`/tmp/grow-ptyctl-cli-smoke.json`。这不能改写默认构建失败结论，也未验证真实 PTY 生命周期。

源码表明登记名称未进行路径检查，固定 tmp 文件和探活后替换缺少并发 CAS。HTTP server 由 axum serve 驱动，CLI 不绑定 graceful shutdown；timeout/linger 的真实作用将继续在 ptyctl/session.rs 核查。上述债务只记录，不混入迁移实现。
