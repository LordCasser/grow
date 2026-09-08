# Verification

- 旧代码运行 same_host_redirect_requires 回归：1 失败，自动请求 /private 并返回 Content。
- 最终 tools web_fetch 全组：130 通过；Shell acp_conversion::tests：29 通过。使用 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p <package> --lib <filter> --quiet`。Shell 仅出现已有 __eh_frame 链接警告，退出码 0。
- 真实 loopback 测试证明同主机路径跳转返回后需独立调用才读取目标；不同端口目标监听器无连接。目标第二次调用正常返回文本。缺失/空 Location、非法 URL、FTP scheme、嵌入凭据均返回 InvalidRedirect。无外部网络。
- ACP 回归验证 Failed 状态、模型新调用提示、目标 URL 和结构化结果序列化往返。枚举 CrossHostRedirect 更名 RedirectRequired，原字段主机扩大为 original_url；无需后向兼容。
- 删除旧自动跳转比较函数及四项锁定旧机制的测试，改用真实 HTTP 行为回归；移除仅服务旧循环的 hop 上限与 TooManyRedirects 错误。
- 工具说明、Shell README 同步。没有新增内部权限回调，重新授权由新的工具调用沿已有入口执行；本轮未用真实模型/UI 发起第二次授权交互。
