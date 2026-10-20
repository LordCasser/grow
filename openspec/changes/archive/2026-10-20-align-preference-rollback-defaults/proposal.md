## Why
补齐交互回归时实际复现：show_tips 成功保存 true 后，重置 false 保存失败，回滚把 true 当成旧默认并清为 None，菜单显示 false。auto_update 的旧回滚默认 false 也与现有菜单默认 true 相反。

## What Changes
把保存/回滚默认值与现有 Settings catalog 保持一致：show_tips=false，auto_update=true。保留未配置镜像和现有顺序保存策略，补充真实 TaskComplete 失败/重试回归。

## Boundary
只修复两个已经确认的默认值漂移，不新增配置项，不改配置文件格式或全局设置框架。不改当前会话权限。
