## 调查边界

从 `SamplingClient` 的 collect、各 backend stream、Chat completion 及 `begin_sideband` 入口核对实际请求构造；不能只搜索 `conversation_collect`，压缩还使用 `chat_completion_stream` 和 `conversation_stream_responses/messages`。Atlas 有范围的 search/calls 只作调用结构证据，输出限于 Focus closure，最终以当前源码核对。

逐个追踪四个问题：原始 ContentPart::Image 是否进入请求；是否选择已确认描述并等待主转换；model/backend/endpoint 是否来自一致配置；拒绝后是主恢复、咨询失败还是静默跳过。既定首次乐观发送、确定的适配遗漏和未复现的路由并发风险分开记录。

复用既有 recap、flush 附件保全及后台压缩路由保护测试；不改已有附件保全断言，不用在线付费模型，不扩大到通用 provider 协议或 reasoning 治理。
