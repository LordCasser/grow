# Why

原生子采样控制回归中的`finished-during-lock`、`mailbox-model-after-finish`、`mailbox-effort-after-finish`在相同前提断言失败：夹具扣住parent usage ACK，并假设此时child actor仍接收命令。当前已归档收尾契约先由child actor提交SessionEnd与Result并拥有shutdown，runner随后才请求parent usage。扣住usage不能保留child mailbox。失败先于目标竞态断言，不能当作越权或新UX回归证据，也不能跳过场景。

# What Changes

只修Shell现测试的边界安排与证据读取，使自然完成在准备/邮箱之后发生时仍能证明拒绝、不回queued、不改变模型或重跑provider。已退役actor的事实从真实持久Timeline读取。需要独立证明actor在execution关闭后的native准入检查时，沿现actor fixture提供精确控制，不强迫生产actor活过其合法终态。

# Impact

纯测试维护，skip_specs。生产收尾顺序、closed-Timeline、工具权限/身份/closed-Step和新控制回执均不改。三个原竞态场景保留，必要的同owner actor控制随本项；不加生产测试开关、通用proxy框架或新的持久事件。验收记录与修正仅在此change，主UX需等待该缺口补验后才宣称完整原生回归通过。
