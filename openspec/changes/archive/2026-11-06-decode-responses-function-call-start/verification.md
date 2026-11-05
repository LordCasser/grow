# Responses initial function-call decoding verification

父从指定会话的原始 SSE 采样证据确认：两次已结束请求的 function_call added 项显式 in_progress、有 name/call_id/id，却缺少 arguments；同会话成功对照含空字符串并完整结束。没有向原 provider 重放请求、执行原工具或改变原会话/Goal。官方示例提供空字段，本变更只描述已观察路由的初始编码差异。

最终唯一生产文件为 sampler/src/client.rs。只对顶层 response.output_item.added + function_call + 显式 in_progress + arguments 键不存在，在供 SDK 解码的副本补空前缀。已有字符串前缀保留，null/非字符串、其他状态、done/terminal 缺参数以及缺身份仍拒收；原始 evidence、usage 明细规范化、L2 身份/前缀/完成验证、最终 JSON 校验和通用 serialization 非重试语义保持。

反例测试还证明初始候选的 Value 重编码会吞掉重复 name/call_id/sequence_number，SDK 原错误被放宽。重复字段置于 item 后面时，SDK 先报 missing arguments，因此只检查第一条错误也不够。最终以两个局部 Default+Deserialize 结构保留已知字段的唯一性，字段值为 IgnoredAny，包含 namespace；nested item 同样可缺省，避免 Option 在 null-first 情况丢失 presence。回归覆盖三种重复、尾部重复、name:null 后重名以及错误 namespace 后重名。它不复制 SDK 类型/必填规则，不建立通用 parser，不改变其他规范化路径。

| 实际验证 | 结果 |
| --- | --- |
| 旧 decoder 上5项新回归 | exit101，2 passed / 3 failed：真实 missing arguments，以及 EOF 被错误归为 Serialization；前两轮测试语法编译错误未计作 red |
| 初始修复上相同5项 | exit0，5 passed，warning0 |
| 仅重复字段控制在初始候选上 | exit101，SDK 拒收而候选接受；保留原 red 与 tail-order red |
| `cargo +1.93.1 test --locked -p sampler --lib -- --test-threads=1` | exit0，265/265，warning0 |
| `cargo +1.93.1 test --locked -p sampler --lib client::tests:: -- --test-threads=1` | 恢复最终快照后 exit0，52/52，warning0 |
| `cargo +1.93.1 test --locked -p sampler --lib stream::responses::tests:: -- --test-threads=1` | 恢复后 exit0，23/23，warning0 |
| `cargo +1.93.1 check --locked -p sampler --lib` | 恢复后 exit0，warning0 |
| `cargo +1.93.1 fmt -p sampler -- --check`、`git diff --check` | exit0 |

成功回归通过实际 HTTP/SSE → SamplingClient.conversation_collect → Responses L2，合成 test_tool 最终参数为 `{"a":1}`，从不执行工具。EOF 控制证明有初始项与完整参数仍不能代替 terminal。已有 client/L2 组覆盖未知顶层事件、已知 malformed、usage、身份/完成冲突等原有边界。

另一次并行全 lib 执行曾264 passed / 1 failed，失败为 auth 测试捕获空 log；该 exact 与最终单线程265项均通过。保留观察，不将原因断言为环境，不改变该日志测试。初始 coder 额外运行完整 sampler integration，前五个二进制6+1+14+1+3通过，test_actor 的六项 terminal-drain 超时。父/收尾 coder 用相同 exact/flags 在 final decoder 与 HEAD 生产 decoder 分别运行，均5.03秒、收到6事件后超时；baseline patch 只有测试区两处 hunk，证明 production 已复原。最终 client 之后逐字恢复、cmp0、前后 hash 相同，再运行上表52/23及生产检查。这证明所选 exact 是既有失败；其余五项没有逐项 baseline 重跑。

源码链路显示 actor 成功发送 RequestCompleted 元数据而旧 test_actor 等待 L2 Completed；该独立测试维护由 release change 的单独 owner 处理，不混入此 decoder 实现。最终全 workspace、发行构建和官方精确提交门禁仍由发布节点验收。

父已核完整初始补丁、最终生产 helper/唯一性字段和新增控制、实际 red/green/baseline 日志及工具链记录，并独立验证当前 client SHA256 `956160bf48034321fd37c2eb23c501941b0bad511e2e5b3194ec88363de293bf` 与最终快照一致；补丁 SHA256 `9b8ce23db629c3a67068ccc39873e5b279ec8d1252bcfe2d5cc7bee3cbb65728`。全部命令使用固定标准 PATH 与 Rust1.93.1。特性已接受，无额外独立 review。原始日志、快照和清单保留于 `/tmp/grow-responses-finish-0WuoPfnJ/`，本记录保留可独立理解的结论与限制。归档前 strict18/18，归档后现行 specs/changes strict17/17、全部archive865/865，均exit0。
