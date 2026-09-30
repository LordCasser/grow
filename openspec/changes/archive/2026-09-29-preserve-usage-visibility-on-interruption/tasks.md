## 1. 取消结算

- [x] 1.1 添加受控 Chat 流回归：确认 finish 与完整 usage 已被解析，再令尾帧 pending 并取消；对照 usage 未到及中间 usage 时取消。记录旧版 `Cancelled → None` 的源码失败路径，再以原数值、缓存 availability、费用和一次 sink 结算作为通过条件。
- [x] 1.2 在单 attempt 内保留 Chat 已确认 usage，并贯穿取消与逻辑期限转换；通过 1.1 与期限回归，验证 provider 尾部持续 pending 时取消仍返回、无额外请求/重试、候选未被接纳。
- [x] 1.3 验证终态同时 ready、结算 ACK pending、未准入取消和 attempt 隔离；确认不会重复结算、先 unknown 后覆盖 known 或借用旧账单。运行现有 Messages/Responses 完整性回归，确认中间 delta 不被提升为精确用量。

## 2. 状态栏与详情

- [x] 2.1 用既有样本计数计算状态栏缓存率，按完整性与 read 覆盖添加前置 `measured cache`；验证完整数据、总量 unknown、部分 read、显式零、write 缺失、无样本、cached/denominator 越界的表驱动场景，保留总量 `≥`。
- [x] 2.2 添加真实累计快照序列回归：已知 A → 多次 unknown steer → 已知 B，以及首样本迟到与子任务结算；断言显示 89.00% 等预期值、incomplete 保留、重复/倒退/replay 快照不重计。与 `/usage` 的样本比例和覆盖率对照。
- [x] 2.3 验证窄屏裁剪与点击范围：百分比可读时 measured 限定仍可见，状态栏可继续打开 Usage，既有 Goal 点击行为通过回归。

## 3. 持久化与预算边界

- [x] 3.1 使用真实 attempt settlement fixture 贯通 Known、Unknown、后续 Known、child fold 和冷恢复，再投影为 `PromptUsage`；验证 lifetime 总数、样本分母及标记一致，重复身份只累计一次，历史 unknown 不被修补。
- [x] 3.2 运行现有未知总消费/精确预算、费用隐藏和 cancellation settlement barrier 的定向测试；确认新展示不清除预算限制，已知取消账单沿全部适用账本 ACK 路径结算。

## 4. 文档、验证与归档

- [x] 4.1 更新 `docs/development.md` 的采样结算与用量展示说明，以及 `crates/codegen/pager/docs/user-guide/04-slash-commands.md` 的 `/usage` 说明；核对 measured、recorded coverage、`≥` 与 delta 一致，实施期间链接本 change。
- [x] 4.2 对修改的 Rust 文件做格式检查，运行 sampler、chat-state、shell、pager 对应上述场景的定向测试，将命令、结果与任何未覆盖边界写入 `verification.md`；按仓库要求限制 Cargo 并发并清理本次不再需要的编译残留。
- [x] 4.3 逐场景核对验证记录后勾选已完成任务，执行 `openspec validate --all --strict --no-interactive`；仅在实现验收完成后归档本 change、更新规范链接，再执行全量规范及 `openspec validate --archived --no-interactive`。
