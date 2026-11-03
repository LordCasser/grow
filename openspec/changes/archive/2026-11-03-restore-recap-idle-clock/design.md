# 设计

在 TimelineBootstrap 被移入 actor 前读取 Existing 的已验证 Timeline，逆序取最近主 Request 事件的 at_ms。Request 的启动、重试、终态均属于主 API 活动；更晚的 Sideband 和 UI 事实不能推迟该时间。初始化原有 AtomicI64，不新增持久字段、计时器或推断标记。没有主 Request 事实则保持 0，并继续不满足自动 idle gate。

现有运行期 Metrics 更新继续负责真实新采样后的时钟。重载不生成 Request、不推进历史，也不把加载耗时当 API 活动。测试以真实主 HTTP 请求生成 canonical Timeline，再增加辅助问答证明恢复时间只依赖主请求；实际 ACP 预建三轮图片会话后，fresh Pager cold load 和真实181秒 away 验证最终自动文字 recap。
