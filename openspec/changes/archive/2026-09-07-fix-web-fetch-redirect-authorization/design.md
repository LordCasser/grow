# Design
preparation 将输入 URL 投影为 WebFetch access，权限层做一次决定；工具资源只提供 WebFetchClient，没有路径或单次许可的传递实体。选用现有跨主机跳转的新调用机制，统一所有目标，以免引入第二个权限回调协议。模型用返回目标发起新调用时，现有一次批准、域名会话许可、deny 和 classifier 照常生效。

HTTP 客户端禁用内建重定向；收到 Location 后解析相对目标并验证 URL 语法、scheme、凭据和长度，返回 RedirectRequired，不发出目标请求。目标 DNS/SSRF 检查发生在新调用中。无自动跳转链，因此删除内部 hop 计数和仅服务该循环的常量/错误；这是修复的一部分，不是删除独立用户功能。ACP 保留未获取内容时 Failed 状态及目标提示。

真实 loopback 服务器记录同主机路径跳转与不同端口目标连接，断言目标无请求；返回的目标经单独调用可读取。验证 malformed Location 和不安全 URL 不会被当作完整页面成功。
