# Verification

- 旧读取路径直接 response.text，全量收集正文；源码证据已确认无读取容量限制。
- 新真实本地 HTTP chunked 测试：空正文、恰好 65536 字节均返回完整文本；65537 字节后服务器不发送结束块且保持连接，读取在外部 1 秒保护内返回 Failed，早于客户端 5 秒 timeout，不等 EOF。
- `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p hooks --quiet`：211 单元、13 集成、1 doctest 全部通过，0 失败/忽略。
- 超限结果不进入 JSON 决策解析，不带正文预览；网络读取超时仍 TimedOut，错误通过 without_url 保持 URL 脱敏。
- 本地 HTTP 测试直接覆盖生产正文读取函数，不放宽 run_http_hook 的 HTTPS URL 校验。未构造本地 TLS 完整 dispatch；现有 dispatcher 失败策略测试仍通过。
- 限制约束应用累积正文，不声称约束底层网络库的单块分配、TLS 或 socket 缓冲。DNS 地址绑定及端到端总时限仍在 backlog。
