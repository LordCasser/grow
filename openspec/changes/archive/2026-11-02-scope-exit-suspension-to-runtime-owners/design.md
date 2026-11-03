退出由 runtime 证明，不再用 attached_as_viewer 推断。客户端在已存在的 ExitSuspension 状态中处理尚无 session identity 的启动；不丢弃 startup owner，不新增自动重试或 timeout。完成退出前在现有输入入口阻止新键盘/粘贴/鼠标 admission；Retry/Force 在 replay 吞键之前处理，Resize 仍正常。leader 沿已有 request id 生命周期跟踪在途 session/new，并结合已有 pending load 表，在返回 session identity 前拒绝退出，响应或断线清除记录。这是请求屏障，不复制 session_driver 所有权。

leader 每次覆盖 exit request 的 leaderScope（连接 id、该连接的 driver session ids），兼容现有 direct/wrapped ext wire forms；客户端自报范围没有权威性。backend 使用 Option<ClientId> 作为 pending exit key，None 表示独立进程，Some 表示 leader 连接；同 scope 在途复用，跨 scope 不复用。断线沿用既有通知释放映射，已启动的 runtime owner 仍归 MvpAgent 管理。

关闭与挂起共享 lifecycle guard：对已退休 actor 的 root 必须确认 draining writer 结束；live root 仍调用同一 tree checkpoint 实现。抽取 guard 内的已有树逻辑以复用，禁止释放 guard 后重新竞争或忽略任意挂起失败。沿用一个现有 5 秒 deadline，未确认仍可显式 Retry/Force。

新增身份之前的后台 startup 与共享 leader 的 request scheduling 需要对抗性核对：至少 bundled Pager 未绑定 startup 不能发出成功退出；不得用缺失 UI view 当作 backend 没有工作。测试通过真实 leader socket 转发验证连接隔离和范围覆盖，Shell 公共 ext_method 验证多 scope、drain 与 retry，Pager 验证 Welcome/viewer、startup、快捷键前缀及既有失败 UI。Tier 3。
