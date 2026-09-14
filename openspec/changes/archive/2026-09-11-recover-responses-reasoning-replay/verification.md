# Verification

## 根因与边界

排队的模型选择在当前 Step 结束后通过 `ReplaceSamplingRoute` 正确开启了新 continuation epoch，并不存在漏调 reset。实际缺口是 portable projector 同时“保留完整结构化工具往返”与“删除全部 reasoning”；DeepSeek Responses thinking + tools 将缺少前置 `reasoning_text` 明确拒绝为 400。

修复不按模型名或 URL 猜测能力。首次精确拒绝在 sampler 边界成为 `portable_responses_reasoning_required` 类型化事实；ChatState 只在当前 backend 为 Responses、portable prefix 含紧邻完整工具往返的非空可见 reasoning、且该 route 尚未启用时确认状态变化。回放 item 不含 id/status/encrypted content。真正 route 替换清除能力；native reset 和同 route 参数更新保留。重复拒绝不再改变状态，自动重提交继续消耗原 logical sampling 的 attempt/deadline。

## 先失败，再修复

- `learned_responses_route_replays_only_visible_portable_reasoning` 在旧 projector 上按预期失败：Responses input 中找不到 reasoning item。
- 修复后该测试验证 `reasoning_text`、function call 与结果各一次，且 Chat Completions、Messages 和普通 final-answer reasoning 不受影响。

## 场景映射

| 契约场景 | 验证 |
| --- | --- |
| queued route switch | `busy_model_and_effort_switch_applies_after_the_active_step` 在活动 Step 中排队 Responses route，边界应用后默认请求删除 reasoning，兼容开启后的重建请求保留它。 |
| 明确 400 分类 | sampling-types 检查精确/近似错误边界；sampler DTO 检查类型化字段及旧 payload 默认值。 |
| 首次恢复及重复终止 | `explicit_reasoning_replay_rejection_updates_once_then_stops` 验证首次返回重提交决策并改变 wire，第二次同错不重新开启恢复。 |
| route 隔离与 reset | ChatState 验证 acknowledged true/false、same-route reset 保留、真实 route 替换清除、无完整工具往返时拒绝启用。 |
| native 与 portable 不重复 | 原有 portable boundary/native span、三 backend switch、完整/歧义工具往返回归全部通过。 |

## 命令与结果

- `RUST_MIN_STACK=16777216 cargo test --quiet --locked --lib -p chat-state -p sampling-types -p sampler -j 2 -- --test-threads=4`：ChatState 478 passed / 1 ignored；Sampler 241 passed；Sampling Types 276 passed；0 failed。
- `RUST_MIN_STACK=16777216 cargo test --locked --lib -p shell session::actor::model_switch::tests -- --test-threads=4`：25 passed，0 failed。
- `RUST_MIN_STACK=16777216 cargo test --locked --lib -p shell session::actor::turn::sampling::image_input_rejection_tests -- --test-threads=4`：7 passed，0 failed。
- `openspec validate --all --strict --no-interactive`：17 passed，0 failed。
- `openspec validate --archived --strict --no-interactive`：320 passed，0 failed。
- Shell 链接保留既有大型 debug 测试二进制的 `__eh_frame` 警告，所有目标测试通过。
- `git diff --check` 通过。

没有调用真实 provider、读取凭据、修改用户会话或发布构建；线上端点行为依据官方协议文档与用户提供的精确错误，mock/单元回归证明本地请求和恢复边界。
