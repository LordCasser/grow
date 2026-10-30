# 验证记录

## 结论与边界

原会话的“输入从约 70 万跳到 137 万并超窗”并非可信的 provider 计量。跨路由保留的最后一组工具调用 ID 触发 neorouter 的 `reasoning_text` 400；Grow 据此补回整段历史可见 reasoning，再用未校准 heuristic 触发压缩。只改变最后一对 call/result ID 的受控变体成功返回 **693,053 input tokens**，而本地压缩使用 **1,370,640**。ID 改写仅用于诊断，没有进入产品。

原失败请求本身没有 usage，不能称其“实际 token 数为 693,053”。该数字来自保留全部历史文本、补回 442 段 reasoning、仅更换最后一对不兼容 ID 并将输出上限设为 1 的受控请求。另一个完全保留原 ID、移除补回的旧 reasoning、追加产品交接提示的变体成功，input 为 **693,085**。两者均非原会话的追加执行，不消费或重执行任何工具。

原配置另有 output_limit=393,216。693,053 + 393,216 = **1,086,269**，超过窗口 1,048,576 **37,693**。所以本次修复解决错误兼容恢复与输入计数跳变，不承诺当前配置下完全不压缩；后续真实校准后的总预算检查仍可合法压缩。没有调整用户配置或放宽准入。

## 原始证据与重建

会话 `01a18aab-acf5-7fb2-a681-38402e52f0cc`，2026-10-30 北京时间：

| Timeline 外层 seq | 时间 | 事实 |
| --- | --- | --- |
| 17020 | 16:31:01 | 旧 Chat 请求 input=692,516，output=10,188 |
| 17042 | 16:31:02 | 新 Responses wire，无 reasoning，output_limit=393,216 |
| 17043 | 16:31:10 | HTTP 400，要求 reasoning_text，usage 缺失 |
| 17052 | 16:31:10 | auto_compact_started：1,370,640 / 1,048,576 |
| 17069 | 16:31:48 | 压缩完成，耗时 37,588 ms |
| 17083 | 16:32:12 | 压缩后成功请求 input=240,057 |

`rebuild_evidence.py` 只读原 Timeline 及 sampling chunk；SurfaceId.event 对应内层 domain seq，不能拿外层 seq/行号代替。全部 1,795 IDs 可解析，442 条非空 reasoning 均能匹配 retained assistant/首 function_call。插入后移除这些 reasoning 项，input 与原拒绝 wire 逐项相等；其他顶层字段也相等。字段形状核对真实后续请求中的 `type,id:null,summary:[],content`。

补回内容共 3,542,788 字符、3,549,097 UTF-8 bytes；逐项 floor(bytes/4) 合计 **887,104**。1,370,640 − 887,104 = **483,536** 仅为算术差额，不能冒充当时独立记录的请求基线。当前代码已包含普通 assistant reasoning 回放修复；历史 2.5.4 请求仍可观察到一条普通回答前 reasoning 未发送。因此重建是完整可见 reasoning 的受控变体，不冒称历史进程实际派发过这份完整请求。

源 wire 与变体 SHA-256、字节数、usage 及失败记录见 `evidence.json`。仓库不保留真实会话正文、推理正文、完整 provider body 或凭据。

## 原端点实验

原 provider 为 neorouter，模型 deepseek-flash，Responses；凭据仅从既有配置在内存读取。计数接口 `/responses/input_tokens` 返回 404。之后所有计量实验使用 stream=false、max_output_tokens=1，保留 tools/function 定义但绝不执行生成调用。返回 incomplete 是主动限制输出的预期结果，不作为成功任务回复写入会话。

1. 完整补回 reasoning 仍是相同 400；给缺正文的 assistant 补 192 个空 reasoning 也失败。失败记录均保留。
2. 使用纯算术内容生成一个正常本路由工具调用，原生回放与剥离 opaque 字段、仅保留可见 reasoning 的回放都成功。保持其 reasoning 不变、换成外部格式 ID 则返回同一 400；使用端点生成的 ID、替换为普通可见 reasoning 则成功。故不能将错误文字简单等同于文本字段缺失。
3. 对原完整会话，只换最后一对 ID，HTTP 200、input=693,053；请求在最后一个 user 后仅有这一组 reasoning/assistant/call/result，所以诊断改动没有重新关联别的调用。
4. 完整 reasoning 加一个明确 user 交接边界：HTTP 200、input=693,060。
5. 与最终修复一致：原 input 无旧 reasoning、全部原工具 ID/结果不变，追加相同交接提示：HTTP 200、input=693,085。

`live_probe.py` 保存单次有界探测方法；`--mode id-control` 仅用于重现实验，产品不会改写 ID。脚本的 handoff 文本已与 Rust 代码核对。早期原生 fixture 强制 tool_choice 被端点拒绝（thinking mode 不支持）；取消强制选择后得到有效原生 fixture，早期失败在 evidence 中单独注明没有保留原 hash。

[DeepSeek Responses 文档](https://api-docs.deepseek.com/guides/responses_api/) 描述可见 reasoning 与相邻 assistant 的合并和 stateless 历史回放。这里的实测结论只归属配置中的 neorouter 端点；不能推广其工具 ID 校验、历史 reasoning 计数行为至所有 Responses provider。

## 代码、回归与对抗性核对

生产变更复用既有 Step 准入和 durable AutoContinue。比较 catalog ID 与实际 transport 字段；effort 等参数变化不触发交接。提示写入位于 owner/取消/手动压缩准入之后、LoopStarted 与 provider dispatch 之前。异步压缩提示优先，避免重复边界。提示不携带 permission_evidence，不是新的真实用户输入。

Rust 1.93.1，`--locked`，独立临时 target，关闭 incremental/debug info。相关回归共 **67 passed，0 failed**：

| crate / 测试过滤器 | 通过数 | 覆盖 |
| --- | ---: | --- |
| shell / route_handoff | 3 | Messages→Responses 工具回合交接一次、原 ID/结果保留、两次工具各执行一次；effort-only 保留原 reasoning；终态切换不重开请求 |
| shell / session::actor::model_switch::tests | 31 | 控制队列、选择生效、路由刷新及故障路径 |
| shell / async_compaction | 18 | 异步提交、已有续接、取消、失败、路由改变、预算与单次提升 |
| shell / compaction_pre_prune_tests::pressure_admission | 9 | 精确窗口边界、输出预留、task clamp、一次压缩与仍超窗终止 |
| chat-state / persistence_failure | 4 | 不确定提交重试、永久故障关闭及现有 rewind/image 持久化回归 |
| chat-state / push_user_message_durably_waits_for_timeline_commit | 1 | durable append 等待提交确认 |
| chat-state / storage_full_failure_poison | 1 | 存储满失败关闭且不无限重试 |

首轮新增测试因只配置一个工具，未匹配 mock 的 foreground 分类，3 个测试均在等待阻塞点超时；已按现有 fixture 的两个工具要求修正，未放宽生产超时。首次使用文件名 `pressure_admission_tests` 过滤没有匹配测试，改为实际模块名后 9 项通过；零项运行未计入验证数量。

对抗性审查逐项核对：控制终止、manual compaction 和 owner 取消先于交接；首 Step index 门禁；异步压缩分支优先于 route 分支；durable append 的错误通过 `?` 在 LoopStarted/provider 之前返回；AutoContinue 构造器的 permission_evidence=None。写入失败和“同时换路由+异步压缩”的新组合没有另做端到端故障注入，由上述分支审查及既有持久化/异步压缩回归覆盖，不冒称所有组合都经过 live 测试。

`cargo fmt --all -- --check`、`git diff --check`、两份审计脚本的 Python AST 解析、诊断 handoff 与生产字符串一致性检查通过。没有运行全 workspace 测试，没有安装或重启正在运行的 Grow。

## 清理与规范校验

归档前全量严格 OpenSpec 校验 17 项通过。32 条脱敏实验记录已逐字段核对原始结果的状态、usage 和已记录哈希/字节数，重建载荷 SHA-256 与长度一致。随后删除本次私有临时目录（包括会话载荷、原始响应和测试日志），只保留脱敏证据与重建方法；原会话未修改。`cargo clean --target-dir /private/tmp/grow-route-handoff-target` 删除 8,104 个文件、2.9 GiB 编译产物。

归档后再次执行 `openspec validate --all --strict --no-interactive`：16 passed；`openspec validate --archived --no-interactive`：746 passed，均无失败。
