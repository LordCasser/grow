# 本机 Agent 协调：发现、执行与恢复边界

这次故障首先是运行时没有启动，而不是目标 Agent 忙。macOS 默认临时目录加上原来的 UUID socket 文件名，会超过 `sockaddr_un.sun_path` 上限。旧实现随后仍暴露工具，枚举退化为空列表，询问退化成泛化错误。`InquiryId` 在寻址前生成，拿到 ID 不代表找到目标。

## 修复后的路径

`CoordinationRuntime` 仍由 Grow agent 进程持有，没有增加中心进程。Unix endpoint 改成 `/tmp` 下随机、`0700` 的短目录和短 socket 名，并在 bind 前校验平台长度。`GROW_HOME` 只决定发现域，不决定 socket 路径。

发现和寻址共用 `live_manifests`。自己的 runtime/租约异常返回 `runtime_unavailable`；目录或清单读取权限错误返回 `discovery_error`，不能解释成“没有其他 Agent”。损坏、已消失、不支持 schema 的清单会跳过。

清单发布串行化整个“快照 + 原子替换”过程，heartbeat 独立于 accept loop。进程终身持有 peer lock；清理必须拿到该锁并重新核对过期记录，不能因一次探测失败删掉活进程清单。清理方不删除别人的 socket，避免 Unix 上路径被 unlink 但旧 listener 仍运行的情况。

询问的执行顺序：

1. 来源先将结构化 `OutgoingStarted` 写入 Timeline 并等待 durable ACK，再解析目标；首次解析的 peer/incarnation 固定在该请求上。
2. 目标先将接收事实写入 Timeline，再进入 Session 的 FIFO 队列；队列最多 32 个等待项，单个 inquiry 执行。
3. 在出队时快照 Surface。同 cwd 自动允许，不同 cwd 要求目标 UI 单次批准；没有在线 UI 则拒绝。自动允许与 UI 审批结果都先进入 Timeline，再决定是否执行旁路请求。
4. 复用 `InfoRequest` sideband，只调用一次模型，不提供工具，不修改主 Surface。主 turn 忙不是拒绝理由。
5. 同来源、同 ID、同 payload 复用运行或结果；payload 不同返回 `conflict`。IPC 断线重连到原 incarnation，发现目标更换进程身份则返回 `target_restarted`，不自动重新执行。

来源清单短暂读失败不会立即取消任务，但也不能续租：只沿用最后一次验证过的 `expiresAt`。明确 Session 关闭、租约到期或取消仍终止询问。

## 工具和状态

`list_active_sessions` 仍只列 primary Session。`ask_session` 新增可选 `inquiry_id`：新询问省略，只有重接同一请求时才传原 ID。`get_inquiry({ inquiry_id })` 查询调用 Session 自己的请求，不允许跨来源读取。

`get` 的 phase 为 `discovering | receiving | queued | awaiting_approval | running | reconnecting | finished`，不是消息阅读回执。终态保留 `answered | rejected | cancelled | unavailable | timed_out | failed`。错误对象包含 `code`、`message`、`retryable` 和可选 `retryAfterMs`，区分 `busy`、`not_found`、`permission_denied`、`transport_error` 等情况。

一个已经终结的 ID 返回缓存结果。对于可重试的终态，应等待建议时间后省略 ID，启动新尝试，不能期待原 ID 自动重新执行。

询问事实复用 Timeline 的 `observation` 事件族，固定为 `coordination/inquiry`，payload 由 `InquiryEvent` 定义。来源保存开始和完成事实，接收侧保存来源身份、接收、审批及终态。内存结果过期或来源进程重启后，`get` 只从经过验证的 Timeline 读取终态；接收侧恢复也只从 actor 持有的 Timeline 找出未闭合询问，追加 `target_restarted`，不重跑模型。第一次终态不可被后续重复记录覆盖。

`UiNotice` 从已提交的询问事实生成，`updates.jsonl` 只是可丢失的显示投影；写入投影失败会记录诊断，不会把已提交的回答改判为审计失败。来源的开始/结束 notice 不额外推送系统通知，重放时也不生成显示行；`list_active_sessions`、`ask_session` 使用普通 ACP 工具调用的等待、完成和展开路径，双击展开工具返回值或错误。

来源还需要完整的 ACP 结果转换：`ListActiveSessions`、`CoordinationInquiry`、`CoordinationInquiryState` 都输出带 `content` 和 `rawOutput` 的 `ToolCallUpdate`。询问失败时状态为 `Failed`，结果查询成功则仍是 `Completed`，即使查询出来的是一个失败的 inquiry。漏掉这个转换时，模型能读到工具结果，但 TUI 只能看到没有正文的占位工具，不能靠修复双击事件解决。

接收侧按 `(sourcePeerId, InquiryId)` 把开始、审批、结束事件投影到同一条工具样式记录，与运行时的去重范围一致。来源身份和结果以 Timeline 为准，并投影到 `UiNotice.details` 的结构化 JSON 中，TUI 再生成可读正文，不从标题猜测身份。开始是 `Answering session <sourceSessionId>`，成功后原位变成 `Answered session <sourceSessionId>`；失败、拒绝、取消和超时使用各自的终态标题。展开后保留来源、工作目录、问题和回答/错误。这个显示行不进入主 turn 的工具 tracker，也不接收主 turn 的工具 hook，所以主 turn 结束不能顺带结束 sideband 的展示。重载时不重复插入同一个 inquiry，旧开始事件不能覆盖终态；只有历史开始记录时，不把它当作仍在线执行的证明。

Minimal 模式的原生终端历史只能追加，不能修改已经打印的行。因此接收侧默认也只展示单行，并且必须等 inquiry 自己的终态再提交到原生历史，不能用“主 Agent 已空闲”推断它完成。执行中的同一行留在 live region，完成后只打印 `Answered` 一次。

Coordination capability、私有 IPC 和 peer manifest schema 现在为 **2**，ACP wire 仍是稳定 **v1**。本次不做旧协调协议兼容，更新后二进制对应的 Grow 进程需要全部重启。

已有运行目录或清单的权限不符合要求时会显式失败，不会自动修改 ACL。需要先确认相关进程已停止，再检查并修复协调运行目录权限；不要把 `active_sessions.json` 当作在线发现数据删除或改写。

## Windows 特有处理

- Named pipe 的 pending server 保留在 listener 内，不能被取消的 `accept()` future 带走。先创建下一实例，再返回当前连接；客户端对 `ERROR_PIPE_BUSY` 做有界重试。
- pipe、目录和清单以 `TokenUser` 的具体 SID 创建私有 DACL，不用可能指向组的 owner 别名。权限在创建时安装，读取清单只校验，不修改 ACL；拒绝 reparse point 和非当前用户的 allow ACE。
- 直接 Win32 文件操作保留 verbatim/UNC 路径前缀。替换清单时只对 sharing/lock violation 做短暂重试；读句柄允许 delete sharing，避免读者阻断发布。
- 清单带 transport namespace，不把 Windows named pipe 与 Unix/WSL socket 当成同一种传输。跨 Windows/WSL 协调仍不在范围内。

## 验证方式

低磁盘占用构建时设置 `CARGO_INCREMENTAL=0`、`CARGO_PROFILE_DEV_DEBUG=0`、`CARGO_PROFILE_TEST_DEBUG=0`；验证完成后执行 `cargo clean`。

```sh
cargo test --locked -p shell -p pager --lib coordination
cargo test --locked -p shell --lib local_ipc
cargo build --locked -p cli --bin grow
python3 scripts/test_local_coordination.py --binary target/debug/grow
```

真实进程脚本使用隔离目录、同一个测试 `GROW_HOME`、两个 `--no-leader` stdio 进程和 loopback 模型，不调用付费模型。不修改子进程默认 `TMPDIR`，因此 macOS 长临时路径条件仍存在。覆盖忙碌主 turn、FIFO、无工具 sideband、同 ID 重试、状态回查、权限、取消、持久化重载和异常退出后的幽灵会话清理。

Windows 新增 accept cancellation、pipe busy、私有 DACL、长路径和持有读句柄时原子替换的原生测试；`.github/workflows/local-coordination.yml` 在 macOS/Linux/Windows 上运行。开发机的 Windows 交叉编译只证明类型检查通过，不能替代原生执行结果。

## 单独处理的边界

- `acp_tool_update` 的通配分支会静默忽略没有接入的新返回值类型；本次只补齐协调工具。其余工具的覆盖审计和穷举约束需要单独处理，不能顺带改变所有工具的通知策略。
- 已完成的询问可以跨来源进程重启回查；尚未持久化终态的询问没有进程崩溃后的 exactly-once 保证。不要自动把它作为新进程的新请求重放。若要提供这种保证，需要单独设计持久化受理/不确定结果语义，而不是把普通重连等同于进程级恢复。
- SIGKILL 可能留下失效 socket 的小型私有目录和 lock 文件。它们不再是在线会话；跨进程回收这些资源必须先证明所有权，不能按路径猜测并删除。本次优先避免误删活 endpoint。
- `GROW_HOME` 的网络文件系统、跨主机、Windows/WSL 混合域和 subagent 独立寻址不在本期范围。父 Agent 的 sideband 可以读取父会话上下文，但不会因此自动拥有子 Agent 的全部内部进度。

### 2026-09-03 展示链路 review 修复

接收行仍走普通工具行的双击展开。修复没有引入第二套审计账本，恢复事实仍写进原有 Timeline，存活状态通过 load 后的瞬时投影重新发布。

- **P1，孤立接收行。** 新 SessionActor 取得独占 writer epoch 后，扫描原有审计的未终结询问，为运行中和排队中的遗留项持久化 `unavailable / target_restarted` 终态。重复 load 不追加第二个终态，也不重新执行模型请求。Minimal 等到这个终态后释放提交位置，不伪造 `Answered`。无法解析结构化身份的历史 notice 保留为普通记录，不持有进行中位置。
- **P2，图片替代正文。** 协调工具与被动接收行使用文本优先展示；本地图片引用仍可识别，但不再把整条结果变成图片块。其他图片工具保留原有展示行为。
- **P2，跨来源 ID 冲突。** 正常更新和增量重载合并都使用 `(sourcePeerId, InquiryId)`，两个独立来源可以使用同一 ID 而不互相覆盖。
- **P2，重连提前结束。** `session/load` 结束前重新发布存活 actor 的未终结接收记录，瞬时投影不再次写入审计、不触发 Notification hook，也不推进重连 cursor。主 turn 的清理跳过被动协调行。真实进程测试还发现 storage 的普通读路径会关闭正在运行的 Sideband；现在只在显式的新 writer load 中执行 Sideband 崩溃恢复，观察性 load 不再修改其 ledger。
- **P2，进行中不能收起。** 接收行在 Answering 和终态都按 Collapsed/Expanded 切换；普通执行工具的 running 折叠策略保持不变。

回归覆盖双击事件、图片路径、来源身份、完整与增量重载、Minimal 提交，以及独立进程中的存活重连和崩溃恢复。代码使用共享 Rust 路径，Windows 也采用这些修复；开发机未执行 Windows 原生 TUI 验证，不能用本机结果替代 Windows CI。
