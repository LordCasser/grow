Grow 已按 Timeline、rewind 锚点、模型路由和 continuation epoch 派生固定长度的普通请求键。同一分支追加消息时该键保持稳定。将键作为 `X-Session-Id` 交给 HTTP 传输，使相同 cwd、系统指令与首消息的独立会话仍能分流。Magpie 的入口会将该头转换为插件使用的 `X-Magpie-Session`。如果路由已显式配置 `X-Session-Id`，Grow 直接发送配置的值。

Sideband 在 run_provider 中使用本次已分配的 Sideband UUID，覆盖整个操作的网络调用。各尝试共享该身份，后台摘要与主会话各有身份。路由作用域属于请求，不能写入进程级默认值或依赖审计是否开启。provider 请求正文与用量保留原语义。重复读文件是现场后果；本次修复作用于分流源头。
