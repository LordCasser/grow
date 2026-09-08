# Why
HTTP Hook 的 DNS 校验与 HTTP 请求各使用完整 timeout_ms，顺序执行使配置的超时不再是整体上限。响应读取阶段超时时也应保留已收到的状态码和原 URL 信息。

# What Changes
使用单个外层 timeout 覆盖 URL 校验、请求与正文，阶段不重置总预算；保留阶段已取得的 HttpInfo。

# Impact
请求自身的超时仍为防御性限制，但外层总预算先结束。失败策略、DNS 地址绑定和正文上限不变。
