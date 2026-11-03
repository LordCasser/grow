## Ownership

ConfigFileWatcher 是已能跨 spawn_blocking 返回的 Send owner。把 watcher 和原来的 UnboundedReceiver 一起交给一个 blocking worker；该 worker 在现有 runtime Handle 上等待队列或取消，顺序执行 native 操作并在同一 worker Drop。返回的 async join observer 只持有 JoinHandle，取消/abort 不把 native watcher 取回 LocalSet。队列结束保留独立 global watch，直到 runtime 取消。

SessionActor 的 ProjectDiscoveryWatcher 使用相同的 owner 边界：ProjectDiscoveryWatcher::start、后续 `refresh_new_dirs` 以及最终 Drop 均留在 blocking worker。worker 在 runtime Handle 上等待 watcher change 或 session cancellation；收到变更后先在 blocking worker 刷新 native watches，再通过无界 actor channel 通知 LocalSet 做异步 skills/workflows reload。SessionActor 不等待初始注册，因此仍可处理安装 coordination backend 的命令 ACK。取消停止 LocalSet 的异步消费并通知 blocking owner；正在执行的 OS 注册不可取消，owner 返回后自行 drop。

该 worker 的 Tokio blocking pool 与 async cancellation observer 分别归属不同 runtime：production session 必须在 `spawn_session_on_thread` 中捕获调用方的 host `Handle`，并通过 `ToolContext::watcher_runtime` 传给 actor；session LocalSet 仍持有 observer，而 `spawn_blocking`、worker 的 `block_on` 和原生 owner drop 归 host runtime。host runtime 另持有一个 Send guardian future：guardian 等待 native worker，并持有预先创建的 worker-cancellation drop guard；LocalSet observer 等待 guardian 的 JoinHandle。session teardown 取消 observer后，其自身 guard 取消 worker；host runtime shutdown 丢弃 guardian时，其 guard也取消 worker。因此 idle worker不会仅因 session observer 尚未取消就永久阻塞 host runtime shutdown。若 native OS 注册调用本身仍阻塞，drop guard只能请求取消，runtime shutdown仍须等该不可取消调用返回。直接通过 `spawn_session_actor` 启动的测试 context 没有 host override 时使用其当前 runtime。

初始已排队注册处理完成后通过 oneshot 通知现有启动逻辑；现有 config consumer 可提前运行，startup all-session reload 在就绪后发送。注册/释放不新增账本、重试墙、线程池或新客户端。

单一 helper 的 owner 泛型只用于对执行边界注入可控的 blocking 操作与 Drop 证据，避免以真实 OS 延迟或 sleep 证明取消安全。生产仍只有 ConfigFileWatcher 这一个 owner。

## Directory identity

ConfigWatchState 复用同模块 discovery 已有的 same_file handle 与 attach_new_refresh_dirs：相同实体不重挂，缺失/替换实体释放原 watch 并在仍有 source owner 时附着新实体。注册来源和常驻/global/explicit 所有权保持。

## Verification

可控阻塞测试证明 queued update 不阻塞本地 future、取消先返回、最终 Drop 在 blocking owner；独立 session runtime 测试在 registration 仍阻塞时完成 writer ACK 和 session runtime shutdown，再释放 native call 并确认 watcher Drop 在线程宿主的 blocking worker。host shutdown 测试保持 session observer 活跃且 native worker idle，验证 host guardian 被丢弃后请求 worker 取消、owner在线程内释放并使 observer结束。现有真实 watcher 组覆盖 creation/recreation/alias/unregistration。Pager 完整 MCP hot-reload 组与公开 session/new/load 验证集成，不加大原期限或跳过失败。

## macOS config event source

完成 offload 后原目录组仍有 2 项事件丢失、Pager MCP 14项中10项失败。notify 8.2.0 的 FSEvents backend 每次 watch/unwatch 都 stop/recreate stream，since_when 固定 SinceNow；新增 cwd 和 .grow 的重挂存在失去期间配置变动的窗口。项目 watch 关闭的全局测试可以通过，单独延后 native 操作不能补回丢失事实。

只对 macOS ConfigFileWatcher 选用 notify 官方 PollWatcher，按现有 debounce 间隔（生产 1 秒）比较已注册非递归目录的元数据，仅对已发现的 config.toml 使用官方内容比较；不读取 cwd 中的其他常规文件，不使用项目树内容扫描、递归项目树、额外重试、全局开关或新依赖。Config/Skills/ProjectDiscovery 复用同一 access-filter wrapper，backend 类型参数仅让 config 的平台选择保持局部；其他 watcher 继续 RecommendedWatcher。保留目录实体与来源 owner 规则以及 blocking-owner Drop。验证仍使用原期限和原事件断言，并重跑 native watcher/MCP 全组。

notify 8.2 的 PollWatcher 默认 mtime 只有整秒，不能单独证明同秒改写/原子替换。macOS config backend 用同一 owner 下的 directory metadata watcher 发现目标，file content watcher 仅哈希 config.toml；它们分别承担发现与内容比较，无第二份 path/owner 账本，外部接口和 event consumer 不变。
