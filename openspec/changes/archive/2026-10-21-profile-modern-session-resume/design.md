## Measurement boundaries

Shell 端到端从 load RPC 发出到 response 返回，包含首条历史前和流式交付；initialize/auth 在计时前。PTY 另测进程启动到真实历史可见及 ready，不能用第一个控制通知当可见历史。父级 inclusive timers 与子级分别报告，不相加；对同一阶段统计 count/sum。

## Fixture

复用既有 SessionSpec、隔离目录与 loopback mock。现代 fixture 每 turn 一个 completed request/attempt 和 admitted assistant，正文为 agent_chunks_per_turn × agent_chunk_len，持久化一个空正文 ordering anchor 与一个由生产 projector 生成的 projection；保留同规模 catalog 与 rewind 负载。可按固定间隔遗漏 projection，anchor 不遗漏，以测 writer load 的 durable repair。生成与正确性 oracle 位于计时外，预算仍为 256 MiB。不假定现代每 turn 的通知数与 legacy chunk fixture 相同。

首轮现代样本还缺少生产新会话已有的模型路由基线，会触发 cold_load_route_baseline 的 durable append。该轮保留为 unseeded 对照，不视为稳态现代恢复。正式现代 E2E 与 PTY 通过生产 model_change_event 构造当前 loopback transport 的合法基线，并检查 load 没有补写 route。GROW_PERF_OMIT_ROUTE_BASELINE=1 可显式重现缺失边界；纯解析阶段无 endpoint 时不虚构 transport。

对照矩阵保留原 legacy 8×4 KiB、legacy 1×32 KiB，以及现代 128/512/2048 turns 的 32 KiB canonical、512 turns 的 4 KiB canonical和每16轮缺一项 projection。legacy 单大 chunk 同时增加 Timeline 正文字节并减少通知数，且原 legacy 无模型路由基线；不把两个 legacy 的总时差当作单变量分片效应。主要归因依据每阶段实际耗时与现代同族对照。

## Attribution

先对照 legacy 与现代完整样本，再比较现代修复样本及规模增长，给出阶段占比和未计时残差。按真实调用判断 R1/R2/S1 是否执行，不能把局部微基准倍数套到总耗时。只有被直接测到的开销可列为主要瓶颈，未测热点保留为假设。完整 before/after 若未复测，明确区分历史局部数据与本轮现代端到端数据。
