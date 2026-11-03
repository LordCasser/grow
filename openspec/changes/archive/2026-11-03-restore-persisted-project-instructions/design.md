## 决策
复用 Timeline 已验证的当前 Surface 与 typed SyntheticReason::ProjectInstructions，按模型顺序聚合正文；单块保持精确字节，与已有 checkpoint 的原规则一致。禁止按标签外形认定来源。source 每次读取权威 ChatState Surface，避免第二份缓存。恢复 spawn 不补入当前磁盘规则；classifier 仅由拥有 permission manager 的 root 设置，新 session 才读取当前 Agent 规则。fresh deferred prefix 保留。

## 不变量与验证
保留原 operation、source gate、writer/权限约束；foreign operation 与真实执行来源漂移仍拒绝。验证 typed 多块/无规则、分支投影、真实持久化后重建与恢复、child 共享 classifier。规则发生显式 reset 时使用其新上下文的规则。

## 独立审查修正：新任务 seed 与恢复不同
child 的持久 seed 原来与恢复共用 Existing 入口。独立审查确认，直接取消 Existing 的补注入会使 fresh child 和 normalized fork 丢规则。以 TimelineBootstrap::Seeded 明确这是新任务的持久初始上下文；仅该入口补入当前规则并持久化，Existing 不补。verbatim mirror 保留原前缀；已有 typed 块不重复注入。restore child 继续 Existing。该区分不新增持久字段、产品配置或规则缓存。
