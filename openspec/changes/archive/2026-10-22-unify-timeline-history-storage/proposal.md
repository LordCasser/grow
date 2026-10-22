## Why

Timeline 与 updates 同时持有正文及展示依据，导致恢复重复解析、构造 digest 和跨文件对账。仅删 projection 正文仍留下两个提交点；直接删 updates 则会丢失顺序锚点和部分唯一历史内容。

## What Changes

- Timeline 保存 canonical 内容、必要的 typed 展示事实和顺序依据，History/Surface 只引用来源；删除 updates 持久化与其 repair/exact-index 路径。
- response admission 与必要展示引用在同一 Timeline 事件确认；候选 anchor 仍先有序持久提交，独立事实和现有 preview budget 约束保留。
- 复用现有 hash 实现为新格式的完整事实编码提供校验，避免依靠第二份正文检测损坏；reader 校验原字节而不重序列化全部 response。
- 统一 cold/resident/child/transcript/fork/import/export 的历史 projector；cursor 绑定 Timeline lineage、截点和派生位置，替换 updates offset。

## Capabilities

### Modified Capabilities

- `session-timeline`：单一事实提交、响应准入、顺序 anchor、正文完整性与恢复身份。
- `client-surfaces`：存储布局、统一派生历史、resident frontier/cursor 与离线 transcript 校验。

## Impact

涉及 chat-state 事件/schema、Shell persistence/storage/replay 与消费者；这是三项中唯一改变持久化及提交契约的 change，应最后实施。必须同项更新 `docs/architecture/agent-core-timeline.md` 和相关开发入口。

仍保留 summary、Sideband、既有 artifacts/rewind 文件及其各自职责。实时 ACP/Grow update 保留。无 SQLite、Git DB、通用事件总线、新 blob/pack、压缩、执行检查点、分页或提前输入。新 schema 明确拒绝旧格式，不双写、不自动破坏性迁移现有会话。实施与验证状态见 verification.md。
