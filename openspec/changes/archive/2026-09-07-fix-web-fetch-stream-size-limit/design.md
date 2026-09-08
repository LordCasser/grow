# Design
fetch_url 为 HTML、文本和媒体共同读取入口。用 Response::chunk 替代 bytes，将累计正文限制在 max_content_length；加入块前用剩余容量判断，避免加法溢出。允许恰好上限与零上限空响应。限制针对 reqwest 解压后的正文，不依赖 Content-Length。网络库单块自身占用不在累计缓冲区上限承诺内。

真实 loopback HTTP 服务提供未结束 chunked 响应；超过上限后客户端必须在服务器关闭前返回大小错误。另验证恰好上限、零容量空/非空。测试显式 no_proxy，不访问外网，不改用户配置。
