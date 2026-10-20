## Design
现有 current_value_for 和 catalog 已使用 show_tips=false / auto_update=true；set_show_tips 的 prev_effective 与 pr13_effective_default 遗留旧值。修正这三处定义，让 first-write rollback 和最近成功值恢复沿同一有效默认。旧默认相关测试改为当前行为契约，同时保留首个默认值提交仍保存、再次相同提交幂等和 None 回滚场景。

使用全表生命周期回归复现失败并证明修复。针对两个 key 的初始失败、最近成功后失败和连续排队失败再做明确断言，不通过跳过、放宽条件或删场景让测试通过。
