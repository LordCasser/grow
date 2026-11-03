## Why

用户报告 Ctrl+X 导引下的操作基本失效。Dashboard 打开的 session 详情和经过 cycle 的多 session 页面，在 AppView 层把 Ctrl+X 截获为 stop/close，导致 AgentView 的配置前缀无法启动。底层 AgentView 单元测试不能发现这个上层冲突。

## What Changes

- session 详情统一先使用已有 Ctrl+X 配置前缀；M/A/E/P/B 路由到当前可用的配置选择器。
- attached root 详情连续第二次 Ctrl+X 在同一个前缀窗口内取消活动 turn 或关闭空闲页面；不同时维护第二份 stop 确认计时。
- child 详情不通过该按键操作父 session。自定义 overlay stop 绑定仍按自己的确认路径工作。
- 更新导引、帮助和真实 AppView 输入回归；Dashboard 列表的停止/删除行为保持本轮边界。

## Capabilities

Modified: `client-surfaces`。

## Impact

Pager AppView 输入路由、AgentView 导引展示与快捷键帮助；没有持久化或 actor 所有权改动。close/delete 的生命周期问题作为后续独立增量处理。
