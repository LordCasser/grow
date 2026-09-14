## Why

用户批准第一批 resume 优化。冷恢复在 storage 已验证 Timeline 后传回 events，actor 再折叠并复制整份 Timeline。现有合成 perf 入口缺少模型配置和真实 Timeline，无法作为有效基线。

## What Changes

- 修复隔离测量 fixture，使用显式 loopback 模型与有界、合法 Timeline 历史，记录优化前后阶段耗时。
- light load 与 actor bootstrap 转移已验证 Timeline 所有权，消除重复 fold；逐一保留 blob/sideband/input/System/control 校验。
- 按实际消费者的需要收窄 bootstrap 临时数据；不为后续 Workflow 恢复长期保留整份 Timeline clone。
- 补充恢复回归及阶段 instrumentation，更新开发者说明。

## Capabilities

不改变会话、恢复、接纳和持久化契约。内部所有权重构与测试/测量维护使用 `skip_specs: true`，不虚构行为 delta。回放交互如需改变行为在独立 change 处理。

## Impact

shell storage/persistence/bootstrap、现有 perf/testkit 与必要调用方。不会复用已退出 resident actor 的快照，不跳过恢复验证，不提前返回 session/load；暂不改 observation/repair 边界。
