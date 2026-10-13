## 1. 查询语义与运行观察

- [x] 1.1 将 session/prompt/subagent 查询与取消发送结果改为 typed outcome，贯穿 Shell、tools 和协议调用方；用表驱动测试验证不存在、通信失败、超时、无效证据和授权拒绝互不折叠。
- [x] 1.2 在现有生命周期 owner/Coordinator 接入紧凑状态快照及退出/panic 更新；阻塞模型、hook 和 persistence ACK 后验证查询无需等待这些操作，且不发布虚假 terminal。
- [x] 1.3 为控制查询落实五秒总期限、每目标单请求、全局并发和有界合并；用虚拟时钟及未退出 worker 验证 deadline 包含排队、超时不释放底层工作许可且重试数量有界。
- [x] 1.4 无 live handle 时接入授权的只读 metadata/Timeline 观察；验证 dormant/失败实例可返回持久 session info，读取不写磁盘、不运行 hook、不竞争 writer lease，读取失败不变为不存在。

## 2. Pager 根与子页面

- [x] 2.1 将失活核对覆盖嵌套 child 及无 current_prompt_id 的 child lifecycle；验证打开子页面、通知丢失和重连均能触发核对，且查询目标身份正确。
- [x] 2.2 贯穿 session、child/prompt、binding epoch 和 incarnation，分开成功观察、实际进展与下次重试时间；验证旧回复不能改新目标，连续 Unknown/Err 不刷新成功时间。
- [x] 2.3 展示运行、结算/提交受阻、交付待确认和观察未知；更新取消 reducer，验证查询失败后可编辑且不自动 drain、不伪造取消成功，真实终态仍只 finalise 一次。

## 3. 集成与归档门槛

- [x] 3.1 用现有 Shell/Pager 测试设施覆盖“执行已返回但 terminal 通知缺失”的根/嵌套场景，以及服务失联和旧实例迟到响应；断言状态在查询期限内可解释、没有重复执行或生命周期事件。
- [x] 3.2 运行受影响的定向测试：`cargo test --locked -p tools --lib <filter>`、`cargo test --locked -p shell --lib <filter>`、`cargo test --locked -p pager --lib <filter>`；在本 change 的 verification.md 记录实际 filter、结果和未覆盖项，避免无关全量构建。
- [x] 3.3 更新 `docs/development.md` 的状态诊断入口并链接归档后契约，核对 delta 的每项 Scenario 有验证记录；运行 `openspec validate --all --strict --no-interactive`，通过后才归档本 change 并执行全量及 archive 校验。
