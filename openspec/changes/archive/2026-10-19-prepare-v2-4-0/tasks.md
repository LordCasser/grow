## 1. Plan and baseline

- [x] 1.1 盘点六个打开 change、已有工作区改动和版本，建立整体顺序及交互不变量。
- [x] 1.2 核对 SDK 接点、各 change 当前实现和调用方，确定共享 schema/owner 的具体落点；MCP 缺口由用户明确选择本版暂缓，见 design 与 verification。

## 2. Implementation and integration

- [x] 2.1 完成本版五个功能 change（MCP 按用户决定暂缓） 的实现和各自验收，保留真实验证证据。
- [x] 2.2 完成跨 change 的冻结、等待、输入、权限、Plan/Goal/Workflow 与恢复联合验证。
- [x] 2.3 对最终实现进行对抗性审查并解决发布阻断问题。
- [x] 2.4 完成 `preserve-input-during-session-bind` 发布阻断修复；真实 worker、Pager 全量、普通/root/tree PTY77、fmt83/strict83 通过，已归档。

## 3. Release preparation

- [x] 3.1 更新 workspace/lockstep 版本至 2.4.0，并编写准确的 release notes 与开发者说明。
- [x] 3.2 完成格式、定向/集成测试、构建及适用发布检查，记录未运行平台验证，检查磁盘及清理本次 Cargo 残留。
- [x] 3.3 完成的功能逐项归档，修正引用及索引；执行全量 strict 和 archive 校验后完成本版本准备。

## 4. Authorized backlog follow-up

- [x] 4.1 逐项对照 backlog 与当前代码、调用方、规范及既有 change，记录确认结果和不属于缺陷的方向/历史事实限制。
- [x] 4.2 对仍存在的问题分别建立或复用最小 change，完成修复、失败路径审查及对应验证；不与并行工作重复立项。
- [x] 4.3 核对新修复与挂起、规划、Plan、Skill 的交互，更新准确的 backlog、版本说明和验证记录；只归档确已完成的 change。

最终运行证据及分层验收范围见 [verification.md](verification.md) 文首；已完成归档；归档后全量与 archive 校验记录见版本联合验证。
