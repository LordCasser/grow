## Context

现有偏好已注册为 Bool；Settings 的值读取、`Action::SetShowModelProvider` 派发、内存渲染缓存、异步持久化和失败回滚均存在。弹窗的 Enter、Space 与鼠标值点击最终使用同一 `toggle_focused_bool` 路径，但 `action_for_bool` 未映射 `show_model_provider`，因此无 Action 发出。现有通用测试 `every_setting_has_action_for_bool_arm` 已可检测这一漏项，之前的定点设置测试只直接派发 Action。

## Goals / Non-Goals

恢复 Settings 行可操作性，并通过弹窗输入路径验证开启与关闭。不新增偏好状态、不更改配置文件格式、模型路由或其他设置行为。

## Decisions

- 在既有 Bool action 映射增加 `show_model_provider`，复用当前 setter 和持久化链路。值来源仍是现有 render cache，弹窗刷新也沿用原路径。
- 在 Settings modal 测试里从真实行聚焦后触发 Enter 和 Space，核对 typed Action 的 true/false 值；保留通用映射完整性测试。鼠标点击共用相同 Bool action 路径，核对该路由而不新增重复的持久化测试。

## Risks / Trade-offs

- 只测直接 dispatch 会再次漏掉弹窗入口；因此回归必须从 `handle_settings_key` 发起。
- 与并行 change 共用工作树；本变更仅编辑 Settings modal 的映射和测试，验证时不覆盖其他未提交文件。
