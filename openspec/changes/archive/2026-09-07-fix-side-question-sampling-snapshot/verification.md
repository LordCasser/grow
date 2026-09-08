## 旧实现失败
受控切换测试 side_question_model_stays_with_prepared_endpoint_after_switch 在旧实现 0 passed / 1 failed：旧 endpoint 收到 new-model。与 recap 共用两个本地 mock 的断言 helper，各自有独立测试和 cfg(test) 一次性切换点。

## 修复范围
从 prepare_chat_completion_config 的同一配置提取 model 后构建 client；删除 /btw 第二次 get_sampling_config。base_request 及 SidebandRoute 使用该 model，现有重试 clone 同一 base_request。没有改变重试次数/分类、上下文来源或工具权限。

## 区别
AI Suggest 默认 grow-build 或客户端 override；Prompt Suggest 使用 catalog 校验的小模型 pin。这两者独立选择模型的规则不能用当前会话模型替代，后续独立检查其路由兼容性。本轮不修改。

## 最终结果
`CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p shell --lib recap --quiet`：57 passed，0 failed/ignored；包含独立 /btw 和 recap 交错测试、既有 side_question 重试分类及次数测试。rustfmt 与 git diff --check 通过。已有 macOS linker unwind 警告，不影响通过。未连接真实 provider。
