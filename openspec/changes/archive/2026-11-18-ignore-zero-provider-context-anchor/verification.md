# Verification

- 按截图时间核对会话的 `sampling_usage` 结算记录。已记录 4,664,269 个输入 token 和 80,194 个输出 token，共 4,744,463。已测缓存读取为 3,883,275，样本命中率为 83.26%。有 3 次用量未知、208 次 provider 显式报告零总量。截图时主回合和一个子任务仍开放。
- 15:53:54 的 Messages 终态报告 164,955 input 和 2,127 output，随后发布 `contextUsedTokens=167082`。15:53:56 至 15:54:01 的相邻响应有工具调用，输入缓存桶与输出均为零，随后发出的上下文 metadata 被清零。该段没有 compaction，模型窗口配置为 1,048,576。
- 回归使用同形状的合成 Messages 事件串。通过生产 Sampler 聚合出带零用量和工具调用的响应，与同一 Shell actor 先前的正数观察及新增上下文对照。修复前 `build_session_info().context.used` 从 169,032 掉到 0，修复后保持 169,032。测试同时确认零用量仍按一次调用结算，累计 token 不变，后续 181,000 的有效观察能更新上下文。
- Rust 1.93.1。Shell 用量回归 14 项通过，窗口准入回归 9 项通过。默认 CLI `cargo check --locked -p cli --bin grow`、全仓格式及严格 OpenSpec 校验通过。会话现场核对只读历史回执，回归未调用实际 provider。
