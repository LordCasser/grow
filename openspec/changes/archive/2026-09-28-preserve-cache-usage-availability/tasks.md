## 1. 协议与类型

- [x] 1.1 核对三协议 wire usage 和锁定 SDK，建立 missing/0/正值、read/write 独立缺失、别名冲突及非法桶的 fixtures；记录每个采用字段的依据。
- [x] 1.2 调整现有 normalized usage 与 mapper 保留字段可用性；验证 Messages start/delta 覆盖、无法推导 full input 以及 Responses 非零 write 的端到端解析。

## 2. 结算与恢复

- [x] 2.1 扩展现有账本的已知缓存累计与覆盖分母，贯通主 attempt、重试、Sideband、child final bill 和 Goal；验证同身份幂等及 availability 冲突的拒绝路径。
- [x] 2.2 更新持久化、cold replay、resume segment 与 provider/model 分组；用相同 fixture 比较 live 和恢复的总量、分类及覆盖，验证 resident reconnect 不重复折叠。
- [x] 2.3 验证精确 Goal 在 full totals 已知/cache 明细缺失时正常按全量计费，full input 本身未知时仍关闭精确预算；覆盖无 Goal、子任务输出额度与迟到结算。

## 3. 展示与验收

- [x] 3.1 更新 `/usage`、normal 状态栏、Goal 详情、ACP/headless 的可用性投影；验证 100/900 输入样例、零分母、read 已知/write 未知与总消费不完整的区别，确认 Goal 只在 read-known attempt 内推导 cache miss，聚合不将未知输入混入该桶。
- [x] 3.2 运行受影响 `sampling-types`、`sampler`、`chat-state`、`shell`、`pager` 的定向与必要综合回归，在 `verification.md` 记录命令、结果、覆盖限制与 Cargo 产物处理。
- [x] 3.3 更新 `docs/development.md` 的计量说明及规范链接，确认没有额外价格推断、provider 策略或账本双写；通过 `openspec validate --all --strict --no-interactive`。
- [x] 3.4 逐项核对全部 delta 场景与验证证据后归档，再执行全量规范和 `openspec validate --archived --no-interactive`。
