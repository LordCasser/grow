# Why
web_fetch 使用 resp.bytes() 完整收集响应后才检查 max_content_length，超大或持续输出的响应会先占用大量内存，配置限制无法约束读取过程。需要在接收过程中执行大小检查。

# What Changes
逐块读取解码后的响应，超过配置上限即返回 ResponseTooLarge，停止继续累积。

# Impact
只修改 web_fetch 正文读取，不改变授权、重定向或媒体处理。
