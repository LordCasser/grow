# 对抗性审查

## 范围与判断

审查未提交的跨路由 AutoContinue 修复；生产 diff 维持 25 行新增、3 行替换，没有扩展 provider、估算或恢复策略。本轮只增加测试与审查记录。

- 路由比较：catalog identity、wire model、backend、base URL、query parameters；不因 effort、输出上限或凭据刷新新增交接。参数比较直接使用已有值，无归一化或路由缓存。模型/Agent 控制队列在同一 Step 边界应用；owner 失效、Goal 预算、行为切换、手动压缩接管与 stationarity 终止先于交接准入。
- 持久化：在既有 step_control_gate 与 owner 准入内等待 durable append，成功后才发 LoopStarted。失败走 fatal turn boundary；不增加重试或旁路。此前只有静态证明，本轮用 TimelinePersistence 在交接消息本身注入 StorageFull。
- 工具与 reasoning：交接追加在完整工具往返之后，不改 ID、参数或结果。旧 lane 已在模型控制阶段转为 portable；新路由收到的下一回复继续形成 native span，不因历史交接被反复清除。新增 wire 顺序及新 reasoning 保留断言。
- 用户轮次与授权：AutoContinue 的 permission_evidence、prompt_index 均为 None，starts_prompt_turn=false。审查 durable append、last_assistant_text_in_turn 和 rewind/truncation 的调用链；没有新增授权或真实 turn。成功用例补充 TurnStarted 计数与字段断言。
- 压缩与计数：交接通过普通消息提交刷新有效压力，后续请求沿既有估算与输出预留门禁；不复用旧路由校准。异步压缩续接分支优先；路由改变导致旧后台摘要失效的现有检查不变。没有放宽窗口或把旧摘要当成新任务。

Atlas scoped search 在该 actor 范围返回覆盖不完整、无匹配；不据此断言无调用方，实际控制流以源码逐段核对。

## 证据复核

独立只读审查确认上次归档没有会话正文、reasoning 正文、工具参数、原始响应或凭据明文；保留会话 UUID、数值与哈希用于溯源。脚本使用显式参数，归档后的相对引用有效。私有路径已按记录删除，无法从仓库独立复算原载荷；本轮不重新发送真实会话。

原失败请求 usage=null，与受控变体 693,053 / 693,060 / 693,085 的作用域区分准确；原 393,216 输出预留仍导致总需求超窗。未将上次的 67 项针对性测试描述成全 workspace 测试。

## 回归结果

本轮 **94 passed，0 failed**；不与上一轮 67 项重复累加。Rust 1.93.1、`--locked`、独立 target、CARGO_INCREMENTAL=0、CARGO_BUILD_JOBS=6、DEV/TEST_DEBUG=0、RUST_MIN_STACK=16777216。

| crate / `cargo test --locked -p <crate> --lib <filter> -- --test-threads=4` | 通过数 |
| --- | ---: |
| shell / route_handoff | 7 |
| shell / session::actor::model_switch::tests | 31 |
| shell / async_compaction | 18 |
| shell / compaction_pre_prune_tests::pressure_admission | 9 |
| chat-state / pressure | 14 |
| chat-state / get_last_assistant_text_in_turn | 2 |
| sampling-types / test_truncate_for_prompt | 13 |

新回归实际验证：交接 StorageFull 保留可重放的已提交前缀且只有一次 StepStarted、没有下一请求；排队切换被取消/手动压缩接管时没有交接；首 Step 以真实用户消息发出一次请求；成功切换只有一个 TurnStarted，原工具调用和结果在交接之前，新路由 reasoning 在再下一 Step 恰好一次。其余回归验证参数更新、native/portable 压力、输出预留、异步压缩失效以及 synthetic 消息的 rewind/回答检索语义。

`cargo fmt --all -- --check`、`git diff --check`、归档 Python 审计脚本 AST 解析及 token 记录作用域核对通过。

## 限制与结论

本轮没有发现需要修改生产代码的新上下文或续接缺陷，新增回归补齐上一轮组合边界的主要空白。该结论限定于已审查路径和本地回归；未运行全 workspace，未覆盖所有 provider 的 live 行为或全部可能的控制并发排列，未重新发送原始会话。生产交接提示、窗口阈值、用户配置及既有恢复策略均未改动。

## 交付校验

归档前 `openspec validate --all --strict --no-interactive`：17 passed。`cargo clean --target-dir /private/tmp/grow-handoff-review-target` 清除 8,229 个文件、3.0 GiB；本轮两份临时编译/测试日志删除，结果保留在上表。最终 diff 范围仅包含原修复、对应规范/开发说明、原调查归档与本轮测试/审查记录。

归档后全量严格校验：Totals: 16 passed, 0 failed (16 items)；archive 校验：Totals: 747 passed, 0 failed (747 items)。
