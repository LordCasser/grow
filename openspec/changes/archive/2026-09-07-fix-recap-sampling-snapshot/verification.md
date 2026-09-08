## 旧实现失败
recap_model_stays_with_prepared_endpoint_after_switch：旧 endpoint 收到 model=new-model，断言 old-model 失败（0 passed / 1 failed）。使用 cfg(test) 一次性切换点和两个本地 MockInferenceServer，不依赖随机调度。

## 修复
提取完整配置准备 helper，原 prepare_chat_completion 委托该 helper；recap 从同一份配置取得 model/window 再构建 client，无第二次 get_sampling_config。凭证刷新、force_http1 和 idle timeout 保留。

## 验证
- Shell recap：56 passed，0 failed/ignored；新测试还验证旧 256k 窗口保留 40KB 输入，即使会话已切到 8k；新 endpoint 未收到该请求，会话配置确已切为 new-model。
- Shell model_switch::tests：24 passed，0 failed。
- 曾尝试 turn::sampling::tests 过滤器命中 0 项，不算验证覆盖，改为实际 model_switch 回归。
- rustfmt 与 git diff --check 通过。
命令均使用 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p shell --lib。已有 linker unwind table 警告未阻止运行。

## 限制
只修复 recap，不声称其他 Sideband 消费者已排除类似窗口；未请求真实 provider。
