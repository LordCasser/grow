# 验证

真实 grow stdio ACP：`test_session_protocol_recovery::auxiliary_image_process_journeys::flush_model_override_resolves_cross_provider_route_for_image_recovery` 通过。主模型与 flush 模型使用独立 loopback server；主路由为 Messages，显式 flush 模型为 ChatCompletions。断言实际目标 endpoint、独立 Authorization、wire model、首次拒图后的 vision 描述与无图重试，以及实际 Markdown 文件含 E42。

actor 回归 `memory_flush_checks_the_actual_override_model_image_key` 与 `unavailable_flush_model_never_falls_back_to_primary_provider` 通过；无效别名没有任何 provider 请求或 memory 文件。全量严格规范校验通过；开发者说明同步到 docs/development.md。

测试用 grow 以 `--no-default-features --features sandbox-enforce` 构建，保留沙箱。默认 jemalloc 的本地 C 生成目录缺头文件，未修改 allocator 或产品配置；已清理本轮失败的 jemalloc 编译目录。
