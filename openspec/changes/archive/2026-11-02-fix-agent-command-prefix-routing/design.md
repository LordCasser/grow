第一按使用 AgentView 现有 leader_key_started_at 及其两秒窗口；attached root 在 AppView 中仅对有效前缀的第二次 Ctrl+X 执行 overlay stop。首按不取消 turn、不启动 close 确认，不和 AppView.pending_action 双重记账。其他后续字母交给 AgentView，child 使用自己的前缀，不以父页面窗口推断权限。

已有自定义 stop key 若不等于 Ctrl+X，继续走注册绑定的确认路径。页面提示明确区分配置前缀与第二次停止/关闭，行为变化更新旧测试预期，保留取消时不关闭工作及子页面不操作父级的不变量。

风险为跨 UI 层的 Tier 2；先让新 AppView 入口测试在旧代码上失败，再改公共路由。已有 Dashboard 基线 669 passed、2 ignored。随后验证 root/child/turn busy、菜单取消和自定义绑定，以及已有导引/overlay/Dashboard 回归。后续再以隔离 PTY 验证完整多 session 旅程。
