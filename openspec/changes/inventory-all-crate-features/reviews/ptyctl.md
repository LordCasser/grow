# ptyctl 逐包核查

包路径：`crates/codegen/ptyctl`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；已运行 cargo test --locked -p ptyctl，15 项测试通过；另以受控短命进程做真实 HTTP/PTy 检查。

## 模块与开关

- `crates/codegen/ptyctl/Cargo.toml`
- `crates/codegen/ptyctl/src/keys.rs`
- `crates/codegen/ptyctl/src/lib.rs`
- `crates/codegen/ptyctl/src/pty.rs`
- `crates/codegen/ptyctl/src/server.rs`
- `crates/codegen/ptyctl/src/session.rs`
- `crates/codegen/ptyctl/src/styled.rs`
- `crates/codegen/ptyctl/src/term.rs`
- `crates/codegen/ptyctl/src/wait.rs`

Cargo feature：`{}`。

## 功能与规范映射

- [Portable PTY spawn and ownership](../specs/pty-control/spec.md#requirement-portable-pty-spawn-and-ownership)：PtyHandle SHALL 通过 portable-pty native system openpty，按 command 数组构建程序与参数，应用 cwd/env，并最后固定 TERM=xterm-256color、COLORTERM=truecolor；保留 master、child、reader、writer。
- [PTY session output processing](../specs/pty-control/spec.md#requirement-pty-session-output-processing)：PtySession SHALL 以阻塞 reader 线程每次最多读取 65536 字节，经无界通道交给 feeder；feeder 先广播原始字节并维护 raw tail，再在 terminal 锁内 feed，最后增加 watch generation。
- [PTY queued input and terminal replies](../specs/pty-control/spec.md#requirement-pty-queued-input-and-terminal-replies)：send_keys SHALL 解析 vim notation 后调用 send_bytes；send_bytes 复制字节到无界 writer channel，writer task 同时接收终端生成的 PtyWrite 响应并执行同步 write_all。
- [PTY child status and ineffective shutdown controls](../specs/pty-control/spec.md#requirement-pty-child-status-and-ineffective-shutdown-controls)：PtySession SHALL 暴露 alive/pid/exit_code 与完整 size/modes/scrollback；当前实现读取 pty 配置启动，但不消费 SessionConfig.timeout 或 linger，stop 只发送内部 shutdown channel 并返回 Ok。
- [PTY resize and grid consistency](../specs/pty-control/spec.md#requirement-pty-resize-and-grid-consistency)：session.resize SHALL 持有 terminal 锁，先同步调整真实 master PTY，再 resize 模拟终端并增加 generation；master 失败则不修改 grid。
- [Vim notation input encoding](../specs/pty-control/spec.md#requirement-vim-notation-input-encoding)：parse_keys SHALL 将普通 Unicode 字符及特殊 notation 交给 terminput 以固定 Xterm 编码；特殊名大小写不敏感，支持 C-/M-/A-/S- 任意顺序修饰。
- [Terminal grid text and modes](../specs/pty-control/spec.md#requirement-terminal-grid-text-and-modes)：Terminal SHALL 使用 alacritty 默认 Config 和 ANSI Processor 增量 feed，导出 1-based cursor、实际 size、scrollback count 及 alt_screen/bracketed_paste/app_cursor/app_keypad/line_wrap/origin/show_cursor/insert/linefeed_newline/focus_in_out/mouse_reporting 模式。
- [Terminal scrollback projection](../specs/pty-control/spec.md#requirement-terminal-scrollback-projection)：scrollback_lines SHALL 最多返回 count 与 history_size 较小值，从最近历史中选取并以最旧到最新顺序输出，offset=1 表示最新历史行。
- [Styled terminal runs and palette](../specs/pty-control/spec.md#requirement-styled-terminal-runs-and-palette)：screen_styled SHALL 按连续相同 cell style 合并 runs，保留绝对行号、fg/bg 与 bold/italic/underline/strikeout/dim/inverse；None 颜色与 false 属性在 JSON 省略。
- [Terminal HTML export](../specs/pty-control/spec.md#requirement-terminal-html-export)：render_html SHALL 生成含内联 CSS 的独立 HTML，按 styled runs 输出颜色、bold/italic/underline/strikeout/dim，并对文本中的 &、<、>、双引号转义。
- [Event driven text and regex waits](../specs/pty-control/spec.md#requirement-event-driven-text-and-regex-waits)：WaitHandle SHALL 在等待前编译 regex，按默认裁剪屏幕以换行拼接检查 Text 子串、Gone 缺失或 Regex；标记 watch generation seen 后检查 grid，条件未满足再等待 generation 或 deadline。
- [Generation based stability waits](../specs/pty-control/spec.md#requirement-generation-based-stability-waits)：StableMs SHALL 以等待开始后的完整无 generation 更新窗口判定稳定，每次 feed 或 resize 的 generation 更新重启窗口，不要求屏幕内容比较后真的变化。
- [PTY HTTP routes and validation](../specs/pty-control/spec.md#requirement-pty-http-routes-and-validation)：build_router SHALL 提供 GET /query/screen、cursor、status、scrollback、/wait，POST /control/send、resize、stop，以及 /ws；使用 very_permissive CORS，本 router 未安装认证或目标 origin 检查。
- [PTY HTTP wait contract](../specs/pty-control/spec.md#requirement-pty-http-wait-contract)：/wait SHALL 恰要求一个 text/regex/gone/stable_ms，默认 timeout_ms=10000、最高 120000，先获取 WaitHandle 再释放 session guard。
- [PTY WebSocket stream and close boundary](../specs/pty-control/spec.md#requirement-pty-websocket-stream-and-close-boundary)：/ws SHALL 订阅连接建立后的输出并发送 binary frames；客户端 binary/raw input JSON/keys JSON/resize JSON 分别入队字节、解析键或调整尺寸。

## 边界

- 索引 command[0] 会 panic，本层没有空命令验证；零尺寸也未显式拒绝。
- master 用 mutex 保护同步 resize；PtyChild 提供 pid/wait/kill，is_alive 将 try_wait 错误当作仍存活。spawn 后 reader/writer 获取错误路径没有显式 kill/wait guard。
- reader 设置 alive=false 并结束；feeder 排空队列后丢弃唯一强 generation sender，使条件等待可观察 ended。
- reader/feeder 队列无界；WebSocket broadcast 容量为 256 个 chunk，慢订阅者可丢输出，不能据此承诺全链路有界或无损。
- 仅表示成功入队，不证明 child 已接收；channel 关闭返回错误。write_all 失败记录 debug 后退出 writer，flush 错误忽略。
- SessionListener 转成字节入队，其余事件在此 listener 忽略；响应仍经同一 writer 转发。
- shutdown receiver 在 start 返回时已丢弃，发送错误被忽略，不调用 child.kill；timeout/linger 不改变生命周期，HTTP stop 的 ok 不能证明停止。
- reader 已设置 alive=false 时 waiter 直接退出，可保持 exit_code=None；alive 是本地观察值，不保证已有完整退出码。
- watch 仍通知等待者重新检查，StableMs 重新计时；feeder 对 SIGWINCH 输出的处理等待同一 terminal 锁。
- 在 terminal 锁内读取 grid 尺寸，不从独立宽高字段拼接；不保证 alive/exit_code 与 grid 属于一个原子快照。
- 生成对应 KeyEvent；只有 Shift 的 ASCII 字母转大写并移除 modifier；不依当前 app_cursor/app_keypad mode 自动选择编码。
- 缺少右括号时左括号按字面量，未知特殊名返回错误，Unsupported 编码静默跳过。特殊名整体 lowercase 且单字符判断使用字节长度，非 ASCII 特殊键没有通用支持。
- 跳过宽字符 spacer，普通 cell 附加 combining 字符；cursor 替换时 plain text 不保留原 combining 字符。默认删尾部空行，所有行仍右 trim；include_empty 只保留空行数量。
- start 按 1-based 饱和减 1，end 原值作为 0-based 排他端并裁到尺寸，实际表达含尾行/列的用户区间；逆序为空。返回 cursor 和 size 保持全终端坐标。
- 跳过 spacer、附加 combining 字符并 trim_end；只投影历史，不把当前屏幕混入，默认 history 策略由 alacritty Config 决定。
- RGB 转六位 hex，named 使用固定 xterm 色表，index 16..231 为色立方、232..255 灰阶；默认前景/背景/cursor 颜色为 None。
- cursor 替换后仍附加原 cell combining 字符；尾部 whitespace run 仅检查 fg/bg None 且 !bold 即删除，未同时检查 italic/inverse 等；include_empty 不关闭 run 级裁剪。
- HTML 不使用 inverse，也不使用传入 cursor/size；存在 cursor CSS 但不自动输出 cursor 标记，cursor_char 只来自前一步文本替换。颜色字符串由 public StyledRun 直接插入 style，库外调用者须控制这些字段。
- generation sender 关闭时提前返回 matched=false、ended=true；匹配时没有 diagnostics。长等待不持有 HTTP 外层 session mutex。
- 返回 screen/cursor/modes/raw_tail/generation/ended 与 elapsed_ms；raw_tail 保留最后 2048 原始字节后 lossy UTF-8 转换，字符串字节数可因替换字符超过 2048，各字段不是同一原子快照。
- 视作一次活动后继续等待完整稳定窗口，预算不足返回诊断；此分支不同于 Text/Gone/Regex 的提前失败。
- 返回 matched=false；库层接受 Duration，没有 HTTP 层的 120 秒 cap，text 条件先检查后等待，因此 timeout 为零也可立即匹配。
- screen styled 返回 JSON runs，html 返回 text/html，其他 format 回退 text JSON；scrollback 默认 100 行并返回 count/lines。
- 冒号区间按含尾边界处理，单值 n 解析 n..n+1，经 resolve_range 实际可选中两行/列；非法值回退全范围，单值 usize::MAX 的 n+1 有溢出风险。
- 分别 HTTP 400/500 JSON error；stop 无条件 ok:true，未实现实际停止。网络绑定由 caller 决定，库层不强制 loopback。
- 返回 HTTP 400 与错误文本；timeout 包含诊断仍是正常 HTTP 200 matched=false，不当成传输错误。
- lag 发送 warning JSON 后继续，丢失 chunk 不重放；坏 JSON 与控制错误忽略，未定义错误 ACK。
- abort 另一任务；只有 broadcast Closed 分支发送 closed/exit_code JSON。PtySession 自身保留 output_tx，WebSocket 又持有 session state，因此 child 退出本身不保证关闭 broadcast 或发出 closed 通知。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。

## 实际验证与差异

macOS 独立工作树执行 `cargo test --locked -p ptyctl`，15 项通过、0 失败，日志 `/tmp/grow-ptyctl-tests.log`。包含真实 shell 的 resize、等待、诊断和 HTTP 等待期间并发操作；没有 Windows 路径或 WebSocket 关闭测试。

补充调用上一阶段显式 reqwest/query feature 构建的 ptyctl CLI，启动仅打印两行后 sleep 3 秒自行退出的 child，配置 timeout=1、不启用 linger。观察到：rows=1 返回两行；stop 返回 ok:true，超过一秒 child 仍 alive；child 自行退出后 HTTP 服务仍可响应，exit_code 为 null。结果 `/tmp/grow-ptyctl-lifecycle-smoke.json`。最后显式结束本次启动的 server 并 wait，未操作现有用户会话。此证据不将默认 CLI 构建失败改写为成功。

WebSocket Closed 分支不可由 child 退出自然触发的问题来自 sender/state 所有权审阅，未做动态 WebSocket 复现。其他未动态复现项包括范围 usize 溢出、启动部分失败回收、无界队列峰值、styled whitespace/inverse 显示差异。均按源码边界写入，未声称测试覆盖所有功能。
