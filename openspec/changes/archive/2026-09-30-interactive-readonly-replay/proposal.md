## Why

现有 replay 复用了正文渲染，却没有完整接入正常会话的详情、鼠标选择和子 agent 浏览。末尾两行长字符串在窄终端会截断；时间固定 UTC；长时间离开、等待输入或关闭后恢复会造成大量无效等待。用户期望的是保留正常会话阅读能力、去掉会话输入及执行能力的播放器。

2026-09-29 的调查与早期代码草稿按用户要求保留，原始状态逐项记在 [handoff.md](handoff.md)。此后按本 change 完成实现；最终验收证据记在 [verification.md](verification.md)。

## What Changes

- 复用既有被动查看组件，补全详情、分组、搜索、文本选择/复制及父子、嵌套子 agent 导航。
- 一次捕获固定会话树，共用播放时钟；切换视图保留阅读位置，不加载未来正文或接入执行通道。
- 用响应式 Replay 面板替代 composer 和 live 状态组件：播放状态/控制、全局进度、当前 agent 路径、历史时间和随焦点变化的操作提示。
- 历史时间按本机在记录时刻的时区显示；无法获取时回退 UTC；估算时间明确标记。
- 已确认的长 IDLE 压缩到固定播放间隔，显示“跳过 xx IDLE 时间”。中断恢复造成的未知空档另标估算；长推理、工具及其他 agent 的真实工作不得因无输出而被自动压缩。
- 保留已有中断/追加输入、Behavior/Plan/Goal/Workflow 的观察性语义；播放器状态不解释为历史业务状态。

## Capabilities

### Modified Capabilities

- `client-surfaces`：扩展只读浏览、固定树播放、底部布局和时间展示；修改旧的默认不压缩 IDLE、顶层退出和粘贴规则，以消除契约冲突。

## Impact

主要涉及 `pager::replay_cmd`、共享被动 viewer/scrollback 组件，以及 `shell::session::storage::transcript` 的只读时间依据提取。复用现有 Timeline，不新增持久化事件，不修改执行、审批、Goal 或 Workflow 状态机。实施时同步更新 CLI 用户指南与开发者说明。

不包括已被用户排除的本机 SIGKILL 排查、安装二进制修复、随机时间 seek/倒放、精确逐 token 原始节奏、通过恢复 live session 补齐历史。历史缺少等待开始/结束事实的限制必须保留在产品和验证说明中，不能猜测。

## Review Package

- [design.md](design.md)：架构、时间轴、IDLE 判定、实现边界。
- [ux.md](ux.md)：ui-ux-pro-max 调研、底部线框、键鼠和焦点规则。
- [audit.md](audit.md)：代码证据、对抗场景和验证矩阵。
- [tasks.md](tasks.md)：顺序实施与验收任务。
- [handoff.md](handoff.md)：保留代码的真实状态、已知阻塞与本轮验证。
- [verification.md](verification.md)：实现后的自动测试、真实 PTY、资源和只读隔离证据。
