# Design
validate_hook_url 返回 (host, Vec<SocketAddr>)。使用 Url::host 的类型化 IPv4/IPv6 分支避免把 IP 当 DNS 名；域名只解析一次，全部地址通过既有 is_blocked_ip 后返回。build_hook_client 使用 resolve_to_addrs 固定连接地址，并 no_proxy 阻止代理端二次解析。URL 本身不替换，TLS SNI、证书名与 Host 仍为原主机；URL 指定端口保持原值。

reqwest 0.13 本地依赖源码确认 resolve_to_addrs 优先于普通解析且 URL 端口优先于 SocketAddr 端口。测试用 reserved .invalid 域名与本地 HTTP socket 证明无需系统 DNS 且 Host 保持；不跳过生产 HTTPS 验证。本次不扩大 IP 分类或处理整体 timeout。
