# 验证与证据

## 真实会话（只读）

会话 `01a15c48-ab5b-7872-a263-15c28b8d8e69`，本机 `.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fvulnerability%2Fphp2shell/` 下同名目录。仅记录结构，不复制正文或认证信息。

- `timeline.jsonl` 5741 / 5749 / 5757 行：同一逻辑步骤的 attempts 1/2/3，路由 DeepSeek `deepseek-v4-flash` / Responses。第三次 provider request_id 为用户截图中的 `f5e6edd5-a9cc-4191-b4d8-9d098395e01b`，400 要求回传 reasoning_text。
- 投影先从 portable_reasoning_backend=None 切到 Responses，再由 portable prefix 714、保留 native spans 重建为 prefix 901、无 native spans；说明现有两级恢复已经执行，仍无法修补 Runtime 模拟调用。
- 请求中 reasoning content 的 type=reasoning_text 项并未全部丢失：三次分别有 49 / 226 / 225 个 reasoning items；native encrypted/identity 在第三次重建中撤销。错误不能简化为“整个请求没传 reasoning”。
- 第三次请求末尾为 Runtime `load_skill` function_call/output，结果字符串长度 5231。用户继续后的首个成功请求（5774 行关联 request）末尾为 user 消息，这组加载 pair 消失；未发现技能正文已转为 user 的证据。之后成功请求有新模型 reasoning 和新 load_skill 调用。
- 这些是结构性根因证据和成功/失败关联，不是服务端校验器内部证据；未向真实 provider 重放用户历史。

## 最小修复与对抗性核对

Responses 用户技能事实直接编码成独立 user 消息，完整 JSON 复用现有 serializer。Model 来源继续输出零额外消息，保留真实调用/结果。无需伪造思考、放宽 400 重试或改变恢复预算。

检查 live、portable prefix、已学习 reasoning 路由、完整 native tool batch 后追加、同一中性批次中插话、原始输入/Timeline 不变。原实现下两个新回归均失败（多出模拟调用/结果）；修复后通过。新增第三项覆盖两个工具结果间插话被延后至完整批次之后。跨 native segment 的插话可达性另外记入 backlog，未混入本次状态机改造。

预算调用方 `chat-state/src/actor/state.rs::estimate_skill_load_tokens` 按完整 JSON 正文加旧工具包装估算；本次仅减少 Responses 包装，仍保守计费，不漏计正文。

Atlas scoped search 提供符号位置；call-graph Focus 查询未在交互窗口完成，调用方/转换边界由直接源码核对，不声称全库调用图完整。

## 执行结果

工具链 Cargo / rustc 1.93.1，与 rust-toolchain.toml 一致。

- 修改前 `cargo test --locked -p sampling-types --lib responses_user_skill -- --test-threads=4`：两个新测试按预期失败。
- 修改后 `cargo test --locked -p sampling-types --lib --quiet -- --test-threads=4`：315 passed，0 failed / ignored。
- `cargo check --locked -p sampling-types`：通过，无诊断。
- 单文件 rustfmt：完成。
- `openspec validate --all --strict --no-interactive`：19 passed / 0 failed（归档前）。

未运行真实 DeepSeek 在线验收，未重建或替换用户已安装的 grow。生产改动只在 sampling-types 私有 Responses 转换中；未改 Shell 恢复、持久化、其他协议或现有工作树的其他功能。

归档后 `openspec validate --all --strict --no-interactive`：18 passed / 0 failed；`openspec validate --archived --no-interactive`：679 passed / 0 failed。归档任务全部完成；验证后定向执行 `cargo clean -p sampling-types` 清理该 crate 编译产物，保留共享依赖缓存。
