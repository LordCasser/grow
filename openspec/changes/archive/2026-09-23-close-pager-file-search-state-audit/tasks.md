## 1. 生命周期回归

- [x] 1.1 为 fuzzy daemon 结果快照携带 query identity，并让 Pager 仅应用当前 query 的快照；以 pager 状态回归测试验证旧请求拒绝和交互状态重置。
- [x] 1.2 收窄 backlog 搜索条目；保留 worker 生命周期审计，删除已处理的 query identity 与 selection/hover/scroll 疑虑。
- [x] 1.3 记录源码调用链、测试和 OpenSpec 校验结果到 verification.md，再归档并复核归档后的规范校验。
