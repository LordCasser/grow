# Verification

- 核对当前 HEAD `3024dad1` 与原有未提交修复；本轮没有更改任何 runtime `.rs` 文件。Atlas 局部 search 未返回 client decoder 符号，后续结论来自源码逐行核对和实际客户端探针，没有把空 Atlas 结果视为不存在实现。
- 只读提取指定会话 Timeline 与响应 artifact；保留原始 HTTP 200、失败决策、unknown usage 和工具未接纳事实。没有发送真实模型请求，也没有改动会话、配置或文件产物。
- `probe.rs` 使用合成 request id 和本机 loopback Axum 服务，通过生产 `SamplingClient::conversation_stream_messages` 入口验证：原始形状精确复现 `missing field type at line 1 column 138`；规范化的 InvalidParameter 为 Fatal(Api 400)；规范化的 overloaded_error 进入现有 retry。
- 同一探针确认 `DataInspectionFailed`、`data_inspection_failed`、`Throttling` 当前错误落入 500/retryable；ServiceUnavailable 为 529/retryable。结果见 `probe-output.txt`。
- 执行方式：临时将本 change 的 `probe.rs` 软链到 `crates/codegen/sampler/examples/incident_missing_type_probe.rs`，运行 `cargo run --locked -p sampler --example incident_missing_type_probe -j 2`；退出码 0。临时入口已在 finally 中删除，没有更改 Cargo.toml 或 Cargo.lock。仅首次构建相关依赖，未跑无关全仓测试。
- 该探针证明错误入口和纯分类规则，不冒充完整 session 自动重试集成测试；本次没有实现新的兼容行为。
- 官方语义依据：[阿里云错误码](https://help.aliyun.com/zh/model-studio/error-code)。实际 code 与官方同文归类之间的差异已在分析中明确。
- `git diff --check` 通过；OpenSpec 归档前 strict 校验 20 passed，归档后全量 strict 校验 19 passed，archive strict 校验 347 passed，均无失败。本分析以 `skip_specs` 归档，没有修改主规范。
