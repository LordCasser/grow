## ADDED Requirements

### Requirement: Interactive doctor collection stays off the UI thread
TUI doctor 报告及修复前采集 SHALL 在受控后台执行，不在 dispatcher 等待 tmux 子进程或文件扫描。

#### Scenario: A diagnostic probe is slow
- **WHEN** 用户请求报告、修复列表或具体修复，某个探测尚未完成
- **THEN** dispatcher 已返回，UI 可继续处理输入，采集并发保持有限。

#### Scenario: Diagnostic origin changes
- **WHEN** 采集完成时原会话已移除、被替换或重新绑定到其他会话
- **THEN** 不将旧报告或修复预览提交到新会话。

#### Scenario: Read-only report and explicit fix
- **WHEN** 报告或修复采集完成
- **THEN** 纯报告不修改配置，具体修复继续经过原有预览和确认流程。
