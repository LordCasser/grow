## 审查方法

以 model-sampling 的跨路由续接、portable 工具配对与上下文压力契约为基准，核对 Step 控制、持久化顺序、prompt index 和权限证据。使用原本地 mock provider 驱动真实 turn，在交接消息提交时注入 StorageFull，并验证没有下一请求；在路由控制排队后撤销 foreground owner，验证不会添加交接或额外采样。补充真实首 Step 和手动压缩接管边界。

不联网重放真实会话，不改原会话，不扩展生产持久化或路由接口。测试故障仅使用现有 TimelinePersistence trait。
