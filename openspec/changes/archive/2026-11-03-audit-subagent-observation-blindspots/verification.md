# 验证记录

2026-11-03，HEAD `bbd2c4dfa` 加当前在制工作树。保留了上一轮修复和其他任务已有改动。本轮最终只交付审查文档及 backlog；不改变产品实现或主规范。

## 方法与限制

工具探针复用真实 TaskOutputTool、ChannelBackend 和既有 coordinator harness；仅 Query 不应答的场景使用受控 responder 保留 oneshot。Shell 探针分别经过公共 ACP handler、生产生命周期 reader，以及 sampler/中断 helper。没有运行模型请求、外部命令或用户的业务任务。

Tokio start_paused 验证的是逻辑时间与 deadline 关系，25s、21s、8s、6s 都不是墙钟基准。原会话只读 probe 针对用户此前提供的 child `01a19f7c-8dbb-7a40-a0e0-8ee35eb79533`，没有构造 writer、恢复执行、修改记录或保存正文。

临时测试在源码中用独立标记块隔离；以下命令在这些块存在时运行。探针确认当前反例后全部移除，不留断言错误行为的普通回归，也不改已有测试预期。现在直接重跑这个 filter 不会复现；正式修复时应按下表在既有 harness 中建立期望行为回归。

## 已执行

`cargo test --locked -p tools --lib audit_observation_probe_ -- --nocapture --test-threads=1`：**8 probes passed**，表示八次诊断的断言命中。

| 场景/输入 | 实际观察 |
| --- | --- |
| 一个真实运行 child；timeout_ms=120000；不发送 finish | 25s 返回 Running，正文却称已等待请求的 120s |
| 两个真实运行 child；timeout_ms=120000；均不发送 finish | 25s 返回，0/2 completed，两项都说 another finished；通用 MultiResult::is_terminal 返回 true |
| healthy + unknown-progress child | healthy 单查询成功；list_running 整份 Unavailable |
| 两个不应答 query；timeout_ms=1000 | 初次 resolve 10s + wait 1s + 再次 resolve 10s，共21s，两项 observation_timed_out |
| coordinator 通道关闭，history reader 带调用计数 | Unavailable，history_calls=0；两个等价探针确认同一条件，不计作两个缺陷 |
| output_unavailable + failed，经公共工具返回 | 两项都有 ended，但 summary 为1/2；通用 helper 不认 output_unavailable/timed_out，私有 summary helper 只认后者 |
| 非阻塞 poll 的 terminal 探测停答 | 六秒外层探针期限到，工具仍未返回，subagent query 尚未发出 |

`cargo test --locked -p shell --lib audit_observation_probe_ -- --nocapture --test-threads=1`：**4 probes passed**。

| 场景 | 实际观察 |
| --- | --- |
| 公共 grow/subagent/get，Query 与 Inspect 各延迟4s | block=false 的完整 handler 在8s成功返回，两阶段没有共同五秒期限 |
| sampler 卡在 signals reply，随后取消 | Weak progress owner 仍可升级；释放停答 actor 后才释放，证明取消未覆盖该等待点 |
| 原会话 child 的 read_durable_subagent_lifecycle | WouldBlock，具体消息为 sampling evidence exceeded its bounded read budget；上游将其作为 Unavailable 提前返回 |
| 中断 `[child-a, child-b]` 的等待 | 合成单项 Result，仅含 child-a，status=running，没有进行状态读取，child-b 不在结构化结果中 |

上述测试均未放宽正常路径断言。虚拟等待只用了测试调度；signals probe 故障由保留 receiver、不启动 actor.run 注入，结束后释放 actor，让 sampler 退出，没有遗留运行时。

## 静态核对

- Atlas 用于局部 query_history 定位；泛型、闭包和同名方法的调用关系以源码为准，不把局部空 callers 当全库无人调用。
- 核对了两个 public ACP 观察步骤、模型 history hook、Pager 实际 RPC、worker 许可、普通 waiter 转移、finalization/retiring、错误和终态投影。
- generic is_terminal 的生产影响、ACP 真实历史往返和其他未跑组合已在 analysis 明确，不扩张成已经发生的调度或权限事故。
- 临时标记块、专用 mock 停答分支和原会话 ID probe 已从产品/测试源移除；代码保留原有修改。复用共享 target，未建立额外 target；incremental 目录仍约36G，未清理其他任务的在用缓存。

`git diff --check` 和本 change 的严格校验通过；3/3 必需工件完整，specs 按 skip_specs 跳过，审查已归档。

归档前后全量严格校验均为22 passed、1 failed，失败项是其他在制 change `fix-frozen-sideband-image-recovery` 的 MODIFIED requirement 遗漏原有场景，位于 local-coordination/model-sampling，不是本审查的行为 delta。本轮未改动该提案或删减原规范来消除失败。归档后 `openspec validate --archived --no-interactive` 为812 passed、0 failed；六条 backlog 证据链接均已核对存在。
