## 1. Inventory

- [x] 1.1 从 Cargo metadata、Git tree/tag、根目录和 CI 枚举全部模块与版本，核对每项都有审查分区和证据位置。
- [x] 1.2 记录当前基线、初始用户改动和活动特性边界，验证报告不把历史证据冒充最终工作树。

## 2. Partition reviews

- [x] 2.1 审视 Session orchestration/Timeline/Workflow，报告当前关键链路及全部标签变化与发现。
- [x] 2.2 审视 Sampling/model/compaction/auth/http，报告当前关键链路及全部标签变化与发现。
- [x] 2.3 审视 tools/permission/sandbox/hooks/MCP，报告当前关键链路及全部标签变化与发现。
- [x] 2.4 审视 Agent/config/plugin/extensions，报告当前关键链路及全部标签变化与发现。
- [x] 2.5 审视 workspace/worktree/index/memory/storage，报告当前关键链路及全部标签变化与发现。
- [x] 2.6 审视 client/UI/terminal/render，报告当前关键链路及全部标签变化与发现。
- [x] 2.7 审视 product/build/platform/tests/vendor/root，报告当前关键链路及全部标签变化与发现。

## 3. Synthesis and verification

- [x] 3.1 架构 owner 核实重要发现和跨区冲突，执行必要定向实验，给出当前缺陷/历史修复/独立债务分类和通过标准。
- [x] 3.2 核对模块×标签覆盖，复核活动特性最终状态，完成总报告与 backlog 引用，不宣称未做的运行验证。
- [x] 3.3 校对链接和证据，执行 OpenSpec 全量 strict、归档及 archive 校验，保留实际结果。
