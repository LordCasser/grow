# Design
私有 read_response_body 接受 reqwest Response，逐 chunk 读取，使用剩余容量比较避免溢出，超过 64 KiB 立即丢弃响应返回 Failed。请求客户端 timeout 继续覆盖读取。IO 错误使用 without_url 和调用方提供的原始日志 URL；正文不写错误。

当前 workspace reqwest 禁用默认特性且未启用 charset，text 使用 from_utf8_lossy，限量读取沿用此语义。超限不可执行部分 JSON 决策；失败由既有 dispatcher 策略处理。响应读取后的 elapsed 取实际完成时间。

本次不处理 DNS 重新解析和 DNS/请求分别计时。测试在本地 HTTP socket 直接调用正文读取，与既有底层客户端测试方式一致，不改变生产 HTTPS 校验。
