## Why

`/btw` 已从 ChatState 原子物化当前 Timeline，但随后按消息类型连续删除尾部 Assistant(tool_calls) 与 ToolResult，连完整工具交换及其正文也一起丢失。连续执行期间，这会让 Sideband 只能看到较早计划，遗漏已经完成的部署或核对证据。截图没有原始请求，因此本次不推断那次会话的具体时间线；修复源码中可确定复现的上下文丢失。

现有 `model-sampling` 只约束 `/btw` 配置快照，没有规定工具证据保留；开发者说明称其共享主上下文，实际组装却无条件裁尾。未归档的 inventory change 只作检索线索，不作为契约依据。

## What Changes

- `/btw` 使用既有 portable history 投影，保留已提交且可正确配对的工具调用、结果、附件和 Assistant 正文。
- 运行中未返回的调用不输出悬空协议，不伪造完成结果；同批已完成的调用保留。
- 增加经过真实 `handle_side_question` 和 loopback provider 的回归，验证实际 wire payload 与主 Surface 隔离；更新原有复制裁尾逻辑的测试。
- 非目标：工具权限、实时读取设备、子任务内部状态聚合、其他 Sideband 的上下文策略、压缩策略与 UI。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `model-sampling`: 补充 side question 对冻结上下文中已完成工具证据的保留契约。

## Impact

影响 `shell/src/session/actor/recap.rs` 的 `/btw` 请求组装及相应测试，复用 `sampling-types::project_portable_history`。不新增实体、依赖、持久化 schema 或 API 字段。开发者入口更新 `docs/architecture/agent-core-timeline.md`。
