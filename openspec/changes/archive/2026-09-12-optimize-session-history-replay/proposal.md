## Why

用户批准第一批恢复体验优化。有效合成样本显示，长会话 actor 恢复和历史回放仍占主要耗时。Grow-only 恢复扫描会反序列化随后丢弃的 ACP 大文本。

## What Changes

- Grow-only 恢复只解析目标方法的 payload，复用现有 rewind 过滤与 pinned reader。
- 分开记录快照读取和过滤；用隔离样本对比完整加载和通知时间，补充协议与输入回归。
- 保留既有初始发送与完成屏障。128 行分批等待方案经测量使完整加载变慢，已撤回；实验结果留在验证记录。

## Capabilities

内部性能重构，不改变既有恢复、输入接纳或持久化契约，使用 `skip_specs: true`。不提前清除 loading_replay，不承诺未经测量的输入延迟。

## Impact

shell storage 的 Grow 投影、MvpAgent 阶段计时及定向测试。cursor、delta/gate 顺序、通知身份、tool-call 合并、rewind 与完整响应屏障保持。读入整份 JSONL 和发送队列的内存上界留待独立测量处理。
