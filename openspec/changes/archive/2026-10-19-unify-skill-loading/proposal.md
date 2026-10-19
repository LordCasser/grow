## Why

当前 `/技能名 参数` 由 Shell 将技能正文拼入用户消息；Agent 自主读取 `SKILL.md` 时，Pager 又根据文件路径显示为 Skill。两条路径的加载语义、失败处理和可见反馈不一致，用户无法从主窗口确认技能是否真正进入了本次上下文。

## What Changes

- 建立统一的 `load_skill(name, args)` 加载入口，继续由现有 SkillManager 提供目录与来源身份。Agent 通过注册工具调用；用户斜杠命令由 Runtime 必定派发，无需先让模型决定是否读取。
- **BREAKING**：斜杠调用不再把技能正文展开到用户输入中。原始用户消息保留，加载结果作为独立、持久化的上下文事实进入后续模型请求。
- 复用现有 Skill 行的折叠、展开和正文查看交互，改由明确的技能身份与 Loading / Loaded / Failed / Cancelled 状态驱动；主窗口直接显示名称和结果。
- **BREAKING**：普通 `read_file(SKILL.md)` 保持文件读取语义，显示为文件读取；路径本身不再证明发生了技能加载。
- **BREAKING**：同一用户输入的技能先准备、再统一发布原输入与独立结果；任一项失败或提交前取消，该组正文和参数都不进入模型上下文。运行中插话失败只拒绝该插话，除非会话持久化本身发生致命错误。
- 优先复用无前置模型请求的 Runtime Step、ToolEvent、现有 Hook 门控和 ACP 行。删除假设的 Hook 参数 / 结果改写能力；显式 Skill 行不参加匿名数量合并。
- 保留命令选择器的显式调用意图，目录在首次接纳前变化也不能退化成普通 prompt。限制正文展开过程及同输入聚合大小，保护新结果直到首次关联模型步骤获得已接纳响应。
- 补齐来源校验、权限与 Hook、队列消费、取消、重复提交、恢复、上下文预算和跨 Provider 投影的行为边界。

## Capabilities

### New Capabilities

无。使用现有能力划分，不建立另一套技能管理或通用强制工具框架。

### Modified Capabilities

- `extension-runtime`：用户与 Agent 共用明确的加载操作，保留发现和预加载的边界。
- `configuration-rules`：选定来源、独立正文结果、引用索引及插件成功诊断。
- `tool-authorization`：注册加载工具，约束 Runtime 强制派发的权限和 Hook 语义。
- `input-admission`：技能作为输入执行前提，覆盖多技能、队列、编辑、插话与失败结算。
- `session-timeline`：加载身份、正文快照、独立上下文结果、幂等提交与只读恢复。
- `model-sampling`：Runtime 来源结果的合法协议投影，以及加载成功的上下文预算边界。
- `context-compaction`：首次关联模型步骤获得已接纳响应前保留完整加载结果；终止该次执行后按既有历史处理。
- `client-surfaces`：复用 Skill 交互、显式状态、普通文件读取与恢复的一致显示。

## Impact

- Shell 的 slash resolver、turn admission、interjection、tool preparation / dispatch、prompt assembly 与会话恢复。
- tools 的 SkillManager、技能内容准备、工具注册与权限元数据；Agent 的技能目录提示及预加载调用方。
- ChatState Timeline、sampling-types 的中性会话事实及各 Provider 请求投影；ACP / headless 与 Pager 的工具显示、分组、回放。
- 后续实现同步更新 `docs/development.md` 和相关架构说明，清理与实际管线不符的旧注释。
- 不新增外部依赖，不改技能发现目录、插件安装、模板语法或 Agent 初始预加载策略，不引入永久“已启用技能”面板。

## Status

2026-10-18：用户授权建立提案并按第二轮审查 R1—R6 修订，先不实现。当前交付范围为 proposal、design、delta specs、tasks、验收矩阵与静态验证记录；产品代码、运行测试、主规范更新与归档均留待后续。修订核对基线为 `4e3cccfa35ed93d6d78e08b5d58838bfaa57b8a2`；审查与关闭记录见 [audit.md](audit.md)。
