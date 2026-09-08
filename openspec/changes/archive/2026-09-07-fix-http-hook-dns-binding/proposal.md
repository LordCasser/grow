# Why
HTTP Hook 在 SSRF 校验阶段解析并检查地址，但 reqwest 发送时再次解析，DNS 变更可绕过已执行的地址检查。自动系统代理还可能把目标解析交给代理，导致本地地址校验与实际连接脱节。

# What Changes
校验返回规范主机名及地址集合，请求客户端固定使用这些地址，禁用自动系统代理，保留原 URL 的 Host/TLS 身份。

# Impact
HTTP Hook 改为直连已校验地址；不再自动采用环境代理。HTTPS 和禁止跳转、现有 IP 分类不变。
