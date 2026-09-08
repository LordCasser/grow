## ADDED Requirements

### Requirement: PTY CLI command routing
ptyctl SHALL 提供 run/send/screen/status/stop/resize/cursor/wait/list 子命令；网络客户端启动前安装 TLS ring provider，CLI 通过 clap 检查必需参数和冲突。

#### Scenario: 目标选择
- **WHEN** 调用控制或查询命令
- **THEN** 必须且只能选择 host、port、name 之一；host 以 http 开头时原样使用，否则添加 http://，port 和登记 name 解析为 loopback URL；不按登记 PID 连接。

#### Scenario: 无命令
- **WHEN** 未提供子命令
- **THEN** 展示帮助并由 clap 返回用法错误。

证据：`crates/codegen/ptyctl-cli/src/cli.rs` — `Commands`；`crates/codegen/ptyctl-cli/src/cli.rs` — `Target`；`crates/codegen/ptyctl-cli/src/cli.rs` — `to_url`；`crates/codegen/ptyctl-cli/src/main.rs` — `install_ring_provider_once`。

### Requirement: PTY CLI launch configuration
run SHALL 默认终端 80x24、loopback 端口 0 自动分配，可指定 cwd、重复 env、name、force、timeout、linger 和 quiet；将 command 原参数数组传给 PtySession。

#### Scenario: 环境参数
- **WHEN** env 含多个 KEY=VAL 或无等号项
- **THEN** 按第一个等号分割，重复 key 后值覆盖前值，无等号项忽略；本层不额外校验空 key、零尺寸或零 timeout。

#### Scenario: 启动顺序和输出
- **WHEN** PtySession 启动成功
- **THEN** 随后绑定 127.0.0.1，登记实际端口；quiet 仅向 stdout 打印端口，否则向 stderr 打印 command/PID/port。Session 启动先于 bind，不能推导端口失败前没有启动 child。

证据：`crates/codegen/ptyctl-cli/src/cli.rs` — `Run`；`crates/codegen/ptyctl-cli/src/commands/run.rs` — `run`。

### Requirement: Named PTY registry persistence
会话登记 SHALL 采用 PTYCTL_SESSION_DIR 优先，否则使用系统 state_dir 或 data_local_dir 下 ptyctl/sessions；保存 port、可选 child pid、command、cwd 和 UTC started_at。

#### Scenario: 写入与查找
- **WHEN** 注册或查找 name
- **THEN** 以 name.json 存储，先写固定 .name.json.tmp 再 rename；不存在或 JSON 无效时报错。name 直接拼入路径，本层未校验路径分隔符或提供并发 CAS。

#### Scenario: 注销与枚举
- **WHEN** 移除或列出登记
- **THEN** 存在时删除；枚举只接受非点开头的 json 且可解析 SessionInfo，跳过坏文件，不保证排序，也不自动清理 dead 登记。

证据：`crates/codegen/ptyctl-cli/src/registry.rs` — `registry_dir`；`crates/codegen/ptyctl-cli/src/registry.rs` — `register_session`；`crates/codegen/ptyctl-cli/src/registry.rs` — `SessionInfo`；`crates/codegen/ptyctl-cli/src/registry.rs` — `list_sessions`。

### Requirement: Named PTY takeover and reachability
未启用 force 时 run SHALL 拒绝 lookup 成功且 server_alive 的同名登记；失效/不可解析登记可替换，force 仅替换登记，不停止旧 server。

#### Scenario: 探活标准
- **WHEN** 查询登记端口
- **THEN** 500ms HTTP client 请求 /query/status，只要求状态恰为 200 且 JSON 存在 size 字段；这不证明进程身份，也不要求 child alive。

#### Scenario: 退出清理
- **WHEN** axum serve 返回后 name 仍指向本次端口
- **THEN** best effort 注销；已指向其他端口则保留。本层没有给 serve 绑定 Session 退出的 graceful shutdown，不能由 timeout/linger 参数推导 HTTP server 会退出。

#### Scenario: 列出状态
- **WHEN** 使用 list 或 list --json
- **THEN** 逐项探测并展示 server 状态；JSON 键为 server_alive，区别于 child alive，空文本列表显示 No active sessions。

证据：`crates/codegen/ptyctl-cli/src/commands/run.rs` — `force`；`crates/codegen/ptyctl-cli/src/commands/run.rs` — `shutdown_result`；`crates/codegen/ptyctl-cli/src/registry.rs` — `server_alive`；`crates/codegen/ptyctl-cli/src/main.rs` — `server_alive`。

### Requirement: PTY CLI keystrokes resize and stop
send SHALL POST /control/send 的 keys，enter 选项追加 <CR>；resize 按小写 x 分割并解析两个 u16，再 POST /control/resize；stop POST /control/stop。

#### Scenario: HTTP 失败
- **WHEN** send/resize/stop 收到非成功状态
- **THEN** 读取响应正文并返回错误；成功 resize 和 stop 分别打印结果，send 不打印成功正文。

#### Scenario: 尺寸用法
- **WHEN** resize 输入不含小写 x 或无法解析 u16
- **THEN** 在发送请求前报错；本层允许零数值，合法尺寸的进一步限制属于服务端。

证据：`crates/codegen/ptyctl-cli/src/commands/client.rs` — `send`；`crates/codegen/ptyctl-cli/src/commands/client.rs` — `resize`；`crates/codegen/ptyctl-cli/src/commands/client.rs` — `stop`。

### Requirement: PTY CLI screen formats and projection
screen SHALL 传递可选 rows/cols/cursor、format 和 full 查询参数；styled/html 打印原响应正文，否则从 JSON lines 数组打印逐行文本。

#### Scenario: 格式选项现状
- **WHEN** 使用 --json 或 --ansi
- **THEN** clap 接受并与其他格式互斥，但 main 忽略这两个字段，实际仍请求 text 并打印 lines；不得将帮助文字当成已实现 JSON/ANSI 输出。

#### Scenario: 行号和缺字段
- **WHEN** 文本格式启用 line_numbers 或响应没有 lines 数组
- **THEN** 行号从本次输出的 1 开始，非字符串项为空串，无 lines 则无输出；styled/html 不加行号。非成功 HTTP 状态报错。

证据：`crates/codegen/ptyctl-cli/src/cli.rs` — `Screen`；`crates/codegen/ptyctl-cli/src/main.rs` — `json: _`；`crates/codegen/ptyctl-cli/src/commands/client.rs` — `screen`。

### Requirement: PTY CLI status and cursor passthrough
status 和 cursor SHALL 分别 GET /query/status 与 /query/cursor 并打印响应文本，网络/读取错误上抛。

#### Scenario: 非成功 HTTP 状态
- **WHEN** 服务端返回可读取的错误正文
- **THEN** 这两个命令没有 error_for_status 或状态检查，仍打印正文并正常返回；不保证 CLI 成功退出表示 HTTP 查询成功。

证据：`crates/codegen/ptyctl-cli/src/commands/client.rs` — `status`；`crates/codegen/ptyctl-cli/src/commands/client.rs` — `cursor`。

### Requirement: PTY CLI condition wait outcomes
wait SHALL 由 clap 要求 text/regex/gone/stable_ms 中一个条件，默认 timeout 10 秒；发起 /wait 请求，timeout_ms 使用饱和乘 1000，HTTP timeout 使用饱和加 5 秒。

#### Scenario: 响应与退出码
- **WHEN** 收到成功 HTTP 状态及可解析 JSON
- **THEN** 完整 pretty JSON 输出到 stdout；matched 为 true 则退出 0，false 或缺失/非布尔则退出 1。

#### Scenario: 错误路径
- **WHEN** 目标解析、连接、非成功 HTTP 或 JSON 解析失败
- **THEN** stderr 打印 Error 并退出 2；等待条件实现及 120 秒服务端上限继续在 ptyctl 库核对，CLI 本身未 clamp 到 120。

证据：`crates/codegen/ptyctl-cli/src/cli.rs` — `Wait`；`crates/codegen/ptyctl-cli/src/commands/client.rs` — `wait`；`crates/codegen/ptyctl-cli/src/main.rs` — `Ok(false) => exit(1)`。

### Requirement: Portable PTY spawn and ownership
PtyHandle SHALL 通过 portable-pty native system openpty，按 command 数组构建程序与参数，应用 cwd/env，并最后固定 TERM=xterm-256color、COLORTERM=truecolor；保留 master、child、reader、writer。

#### Scenario: 输入前提
- **WHEN** 直接库调用提供空 command
- **THEN** 索引 command[0] 会 panic，本层没有空命令验证；零尺寸也未显式拒绝。

#### Scenario: 拆分与回收
- **WHEN** into_parts 交出 master/child/reader/writer
- **THEN** master 用 mutex 保护同步 resize；PtyChild 提供 pid/wait/kill，is_alive 将 try_wait 错误当作仍存活。spawn 后 reader/writer 获取错误路径没有显式 kill/wait guard。

证据：`crates/codegen/ptyctl/src/pty.rs` — `PtyHandle`；`crates/codegen/ptyctl/src/pty.rs` — `spawn`；`crates/codegen/ptyctl/src/pty.rs` — `PtyChild`；`crates/codegen/ptyctl/src/pty.rs` — `into_parts`。

### Requirement: PTY session output processing
PtySession SHALL 以阻塞 reader 线程每次最多读取 65536 字节，经无界通道交给 feeder；feeder 先广播原始字节并维护 raw tail，再在 terminal 锁内 feed，最后增加 watch generation。

#### Scenario: 流结束
- **WHEN** reader EOF 或非 WouldBlock 错误
- **THEN** reader 设置 alive=false 并结束；feeder 排空队列后丢弃唯一强 generation sender，使条件等待可观察 ended。

#### Scenario: 缓冲边界
- **WHEN** 输出速率超过 feeder 或订阅者
- **THEN** reader/feeder 队列无界；WebSocket broadcast 容量为 256 个 chunk，慢订阅者可丢输出，不能据此承诺全链路有界或无损。

证据：`crates/codegen/ptyctl/src/session.rs` — `PtySession`；`crates/codegen/ptyctl/src/session.rs` — `pty_read_tx`；`crates/codegen/ptyctl/src/session.rs` — `generation_tx.send_modify`。

### Requirement: PTY queued input and terminal replies
send_keys SHALL 解析 vim notation 后调用 send_bytes；send_bytes 复制字节到无界 writer channel，writer task 同时接收终端生成的 PtyWrite 响应并执行同步 write_all。

#### Scenario: 调用成功
- **WHEN** send_bytes 返回 Ok
- **THEN** 仅表示成功入队，不证明 child 已接收；channel 关闭返回错误。write_all 失败记录 debug 后退出 writer，flush 错误忽略。

#### Scenario: 设备查询回应
- **WHEN** alacritty 发出 Event::PtyWrite
- **THEN** SessionListener 转成字节入队，其余事件在此 listener 忽略；响应仍经同一 writer 转发。

证据：`crates/codegen/ptyctl/src/session.rs` — `send_bytes`；`crates/codegen/ptyctl/src/session.rs` — `pty_response_rx`；`crates/codegen/ptyctl/src/term.rs` — `SessionListener`。

### Requirement: PTY child status and ineffective shutdown controls
PtySession SHALL 暴露 alive/pid/exit_code 与完整 size/modes/scrollback；当前实现读取 pty 配置启动，但不消费 SessionConfig.timeout 或 linger，stop 只发送内部 shutdown channel 并返回 Ok。

#### Scenario: stop 或 timeout
- **WHEN** 调用 stop 或经过配置 timeout
- **THEN** shutdown receiver 在 start 返回时已丢弃，发送错误被忽略，不调用 child.kill；timeout/linger 不改变生命周期，HTTP stop 的 ok 不能证明停止。

#### Scenario: 退出状态竞态
- **WHEN** reader 先于 100ms waiter 观察 child 退出
- **THEN** reader 已设置 alive=false 时 waiter 直接退出，可保持 exit_code=None；alive 是本地观察值，不保证已有完整退出码。

证据：`crates/codegen/ptyctl/src/session.rs` — `SessionConfig`；`crates/codegen/ptyctl/src/session.rs` — `stop`；`crates/codegen/ptyctl/src/session.rs` — `alive_waiter`。

### Requirement: PTY resize and grid consistency
session.resize SHALL 持有 terminal 锁，先同步调整真实 master PTY，再 resize 模拟终端并增加 generation；master 失败则不修改 grid。

#### Scenario: 运行中的等待
- **WHEN** resize 引发 reflow/clipping 但 child 没有输出
- **THEN** watch 仍通知等待者重新检查，StableMs 重新计时；feeder 对 SIGWINCH 输出的处理等待同一 terminal 锁。

#### Scenario: 查询尺寸
- **WHEN** status 读取 size
- **THEN** 在 terminal 锁内读取 grid 尺寸，不从独立宽高字段拼接；不保证 alive/exit_code 与 grid 属于一个原子快照。

证据：`crates/codegen/ptyctl/src/session.rs` — `resize`；`crates/codegen/ptyctl/src/pty.rs` — `PtyMaster`；`crates/codegen/ptyctl/src/session.rs` — `status`。

### Requirement: Vim notation input encoding
parse_keys SHALL 将普通 Unicode 字符及特殊 notation 交给 terminput 以固定 Xterm 编码；特殊名大小写不敏感，支持 C-/M-/A-/S- 任意顺序修饰。

#### Scenario: 键集合
- **WHEN** 使用 Enter/CR/Return、Esc/Escape、BS/Backspace、Tab、Space/Spc、方向/Home/End、PageUp/PgUp、PageDown/PgDn、Insert/Ins、Delete/Del、F1..F12、lt/gt/bar/bslash 或单字节字符
- **THEN** 生成对应 KeyEvent；只有 Shift 的 ASCII 字母转大写并移除 modifier；不依当前 app_cursor/app_keypad mode 自动选择编码。

#### Scenario: 错误与字面量
- **WHEN** 缺少右尖括号、未知特殊名或 encoder Unsupported
- **THEN** 缺少右括号时左括号按字面量，未知特殊名返回错误，Unsupported 编码静默跳过。特殊名整体 lowercase 且单字符判断使用字节长度，非 ASCII 特殊键没有通用支持。

证据：`crates/codegen/ptyctl/src/keys.rs` — `parse_keys`；`crates/codegen/ptyctl/src/keys.rs` — `parse_to_events`；`crates/codegen/ptyctl/src/keys.rs` — `parse_special`。

### Requirement: Terminal grid text and modes
Terminal SHALL 使用 alacritty 默认 Config 和 ANSI Processor 增量 feed，导出 1-based cursor、实际 size、scrollback count 及 alt_screen/bracketed_paste/app_cursor/app_keypad/line_wrap/origin/show_cursor/insert/linefeed_newline/focus_in_out/mouse_reporting 模式。

#### Scenario: 屏幕文本
- **WHEN** screen_content 指定选区和可选 cursor_char
- **THEN** 跳过宽字符 spacer，普通 cell 附加 combining 字符；cursor 替换时 plain text 不保留原 combining 字符。默认删尾部空行，所有行仍右 trim；include_empty 只保留空行数量。

#### Scenario: 选区
- **WHEN** 传入 ScreenOpts 的 Range
- **THEN** start 按 1-based 饱和减 1，end 原值作为 0-based 排他端并裁到尺寸，实际表达含尾行/列的用户区间；逆序为空。返回 cursor 和 size 保持全终端坐标。

证据：`crates/codegen/ptyctl/src/term.rs` — `Terminal`；`crates/codegen/ptyctl/src/term.rs` — `screen_content`；`crates/codegen/ptyctl/src/term.rs` — `resolve_range`；`crates/codegen/ptyctl/src/term.rs` — `terminal_modes`。

### Requirement: Terminal scrollback projection
scrollback_lines SHALL 最多返回 count 与 history_size 较小值，从最近历史中选取并以最旧到最新顺序输出，offset=1 表示最新历史行。

#### Scenario: 宽字符与空白
- **WHEN** 读取历史行
- **THEN** 跳过 spacer、附加 combining 字符并 trim_end；只投影历史，不把当前屏幕混入，默认 history 策略由 alacritty Config 决定。

证据：`crates/codegen/ptyctl/src/term.rs` — `scrollback_lines`；`crates/codegen/ptyctl/src/term.rs` — `ScrollbackLine`。

### Requirement: Styled terminal runs and palette
screen_styled SHALL 按连续相同 cell style 合并 runs，保留绝对行号、fg/bg 与 bold/italic/underline/strikeout/dim/inverse；None 颜色与 false 属性在 JSON 省略。

#### Scenario: 颜色
- **WHEN** 终端使用 RGB、named 或 indexed color
- **THEN** RGB 转六位 hex，named 使用固定 xterm 色表，index 16..231 为色立方、232..255 灰阶；默认前景/背景/cursor 颜色为 None。

#### Scenario: 裁剪与空格
- **WHEN** 处理 styled cursor 或尾部空白 run
- **THEN** cursor 替换后仍附加原 cell combining 字符；尾部 whitespace run 仅检查 fg/bg None 且 !bold 即删除，未同时检查 italic/inverse 等；include_empty 不关闭 run 级裁剪。

证据：`crates/codegen/ptyctl/src/term.rs` — `screen_styled`；`crates/codegen/ptyctl/src/styled.rs` — `extract_styled_line`；`crates/codegen/ptyctl/src/styled.rs` — `color_to_css`；`crates/codegen/ptyctl/src/styled.rs` — `indexed_color_to_css`。

### Requirement: Terminal HTML export
render_html SHALL 生成含内联 CSS 的独立 HTML，按 styled runs 输出颜色、bold/italic/underline/strikeout/dim，并对文本中的 &、<、>、双引号转义。

#### Scenario: 投影限制
- **WHEN** 输入含 inverse、cursor 坐标或尺寸
- **THEN** HTML 不使用 inverse，也不使用传入 cursor/size；存在 cursor CSS 但不自动输出 cursor 标记，cursor_char 只来自前一步文本替换。颜色字符串由 public StyledRun 直接插入 style，库外调用者须控制这些字段。

证据：`crates/codegen/ptyctl/src/styled.rs` — `render_html`；`crates/codegen/ptyctl/src/styled.rs` — `html_escape`。

### Requirement: Event driven text and regex waits
WaitHandle SHALL 在等待前编译 regex，按默认裁剪屏幕以换行拼接检查 Text 子串、Gone 缺失或 Regex；标记 watch generation seen 后检查 grid，条件未满足再等待 generation 或 deadline。

#### Scenario: 锁与结束
- **WHEN** 终端输出结束且条件未满足
- **THEN** generation sender 关闭时提前返回 matched=false、ended=true；匹配时没有 diagnostics。长等待不持有 HTTP 外层 session mutex。

#### Scenario: 诊断
- **WHEN** 超时或提前结束
- **THEN** 返回 screen/cursor/modes/raw_tail/generation/ended 与 elapsed_ms；raw_tail 保留最后 2048 原始字节后 lossy UTF-8 转换，字符串字节数可因替换字符超过 2048，各字段不是同一原子快照。

证据：`crates/codegen/ptyctl/src/wait.rs` — `wait_for`；`crates/codegen/ptyctl/src/wait.rs` — `timeout_outcome`；`crates/codegen/ptyctl/src/wait.rs` — `push_raw_tail`；`crates/codegen/ptyctl/src/session.rs` — `wait_handle`。

### Requirement: Generation based stability waits
StableMs SHALL 以等待开始后的完整无 generation 更新窗口判定稳定，每次 feed 或 resize 的 generation 更新重启窗口，不要求屏幕内容比较后真的变化。

#### Scenario: 输出结束
- **WHEN** generation sender 被丢弃
- **THEN** 视作一次活动后继续等待完整稳定窗口，预算不足返回诊断；此分支不同于 Text/Gone/Regex 的提前失败。

#### Scenario: 超时边界
- **WHEN** window 超过剩余 deadline
- **THEN** 返回 matched=false；库层接受 Duration，没有 HTTP 层的 120 秒 cap，text 条件先检查后等待，因此 timeout 为零也可立即匹配。

证据：`crates/codegen/ptyctl/src/wait.rs` — `wait_stable`；`crates/codegen/ptyctl/src/wait.rs` — `WaitCondition`。

### Requirement: PTY HTTP routes and validation
build_router SHALL 提供 GET /query/screen、cursor、status、scrollback、/wait，POST /control/send、resize、stop，以及 /ws；使用 very_permissive CORS，本 router 未安装认证或目标 origin 检查。

#### Scenario: 屏幕和历史
- **WHEN** 请求 screen 或 scrollback
- **THEN** screen styled 返回 JSON runs，html 返回 text/html，其他 format 回退 text JSON；scrollback 默认 100 行并返回 count/lines。

#### Scenario: 范围解析现状
- **WHEN** range 为 1:5、5:、:10、5 或非法文本
- **THEN** 冒号区间按含尾边界处理，单值 n 解析 n..n+1，经 resolve_range 实际可选中两行/列；非法值回退全范围，单值 usize::MAX 的 n+1 有溢出风险。

#### Scenario: 控制结果
- **WHEN** send 解析失败或 resize 操作失败
- **THEN** 分别 HTTP 400/500 JSON error；stop 无条件 ok:true，未实现实际停止。网络绑定由 caller 决定，库层不强制 loopback。

证据：`crates/codegen/ptyctl/src/server.rs` — `build_router`；`crates/codegen/ptyctl/src/server.rs` — `parse_range`；`crates/codegen/ptyctl/src/server.rs` — `handle_screen`；`crates/codegen/ptyctl/src/server.rs` — `handle_send`；`crates/codegen/ptyctl/src/server.rs` — `handle_stop`。

### Requirement: PTY HTTP wait contract
/wait SHALL 恰要求一个 text/regex/gone/stable_ms，默认 timeout_ms=10000、最高 120000，先获取 WaitHandle 再释放 session guard。

#### Scenario: 用法错误
- **WHEN** 零/多个条件或非法 regex
- **THEN** 返回 HTTP 400 与错误文本；timeout 包含诊断仍是正常 HTTP 200 matched=false，不当成传输错误。

证据：`crates/codegen/ptyctl/src/server.rs` — `handle_wait`；`crates/codegen/ptyctl/src/server.rs` — `WAIT_MAX_TIMEOUT_MS`。

### Requirement: PTY WebSocket stream and close boundary
/ws SHALL 订阅连接建立后的输出并发送 binary frames；客户端 binary/raw input JSON/keys JSON/resize JSON 分别入队字节、解析键或调整尺寸。

#### Scenario: 慢客户端或错误输入
- **WHEN** broadcast lag 或无法解析的输入
- **THEN** lag 发送 warning JSON 后继续，丢失 chunk 不重放；坏 JSON 与控制错误忽略，未定义错误 ACK。

#### Scenario: 任务与结束
- **WHEN** 任一收发任务结束
- **THEN** abort 另一任务；只有 broadcast Closed 分支发送 closed/exit_code JSON。PtySession 自身保留 output_tx，WebSocket 又持有 session state，因此 child 退出本身不保证关闭 broadcast 或发出 closed 通知。

证据：`crates/codegen/ptyctl/src/server.rs` — `handle_ws_connection`；`crates/codegen/ptyctl/src/server.rs` — `WsClientMessage`；`crates/codegen/ptyctl/src/session.rs` — `output_tx`。

