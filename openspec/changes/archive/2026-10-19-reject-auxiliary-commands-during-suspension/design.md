# Design

冻结是准入关闭与已接纳 owner 排空的组合，不是立即销毁 actor 邮箱。`request_node_suspension` 接纳有效原操作身份后应同步关闭顶层 activity gate，再启动异步 SuspendRequested/checkpoint worker。旧 owner 的嵌套 Sideband/用量结算继续使用既有 start_nested，不重新打开 gate。

保持现有 tracker，不新增命令分类框架。邮箱中每个原本 try_start(...).expect(...) 的有限命令，都把 None 视为正常的关闭边界。对配置类命令，permit 获取移到该分支的异步工作/副作用之前；拒绝就不执行该命令。没有被分派的 best-effort suggestion 返回 None；其他 RPC 沿现有关闭 responder 语义结束，不伪造成功。Runtime 状态观察及挂起/恢复控制、已有用量和持久化 ACK 不经这个有限工作门控。

原始会话记录能证明 panic 和未完成挂起，不能单凭截图证明长时间停顿全部来自同一处。测试需在真实 run_session mailbox 中先保持一个已接纳 owner，接纳挂起、提交迟到补全和查询，验证 actor 活着、补全无新 provider 请求、checkpoint 不早于 owner 排空。随后释放 owner，验证同一操作 checkpoint、等待方和物理 retire 完成。另在挂起请求 ACK 阻塞期间验证早期准入关闭；不通过手动 Idle、替换 mailbox 或仅测试 tracker 来制造成功。

运行本轮精确恢复测试、Shell/Tools 相关测试及真实 PTY 树恢复；必要时增加超时未确认的 mailbox 交错。长会话的 UI 卡顿若有独立证据，另行记录，不在本 change 混入泛化性能重构。

排空已有 owner 可能越过调用方 deadline。保持 ChatState 对已派发/不明确写入的失败隔离契约；在 Session 层完成无界物理排空后检查剩余期限，过期则在发出 checkpoint 相关写入前返回未确认，供同一 operation 用新期限重试。不能拿旧期限制造尚未发出的 pending commit，导致健康 writer 被关闭。
