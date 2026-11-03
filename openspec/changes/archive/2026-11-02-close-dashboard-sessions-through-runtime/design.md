使用现有 grow/session/close，而非新增 close 协议或把 UnregisterActiveSession 当执行终止。已绑定页面进入 AgentCommand::CloseSession，异步 effect 携带原 AgentId、session id 与 binding epoch；成功且绑定仍匹配时才移除页面，失败保留关闭状态并允许用户重新确认关闭。没有自动重试或独立超时墙。

页面退休前调用既有 local draft sync 捕获最新内容；现有 worker 的 key-transition boundary 与 closed-owner flush 保留保存责任，不新建草稿账本。未绑定且已失败的临时页面可本地移除；仍在创建/加载的页面须保留原启动 owner，不能丢掉在途创建回执。

Dashboard 关闭中的行可重试同一逻辑 session close，其他 Working 行走已有停止活动计划，后端确认前不允许删除。close result 只有在原绑定仍匹配时更新页面与反馈；晚到结果不移除后来绑定的新 session。最后页面移除时的落点使用现有 Dashboard/Welcome，不再以页面数量拒绝。

Tier 2，并验证真实服务端 close 的 writer drain/历史保留、失败和迟到回执、草稿及 last-view 场景。进程退出与并发 close 的 drain 作用域需要在后续 owner-scope 增量统一验证，不能把本轮页面闭环当成全旅程完成。
