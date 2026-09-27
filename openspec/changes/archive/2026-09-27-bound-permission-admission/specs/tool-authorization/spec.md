## ADDED Requirements

### Requirement: Permission fanout has finite fail-fast admission

每个主会话共享的 child 权限请求、主 Agent 权限请求与权限 Sideband 活跃数 SHALL 有固定有限上限；达到对应上限 SHALL 立即返回不授权的超载结果，而不是把新的模型/人工等待放入无界队列。child 请求饱和 SHALL 仍为主 Agent 保留有限准入容量。已准入的模型裁决 SHALL 继续独立并行；调用方取消后，actor 丢弃该请求或完成裁决时 SHALL 释放容量。控制撤销和关闭命令 SHALL 在请求饱和时仍可处理。超载 SHALL 以不含请求参数的固定标签指标和日志可观测。

#### Scenario: Many child requests are pending
- **WHEN** 共享权限 manager 的活跃请求达到上限，另一个 child 提交工具调用
- **THEN** 新调用及时收到不授权结果；旧调用仍能独立完成或被取消，控制命令仍可执行。

#### Scenario: The primary continues while children saturate admission
- **WHEN** child 权限请求占满准入容量，主 Agent 提交本地权限决策
- **THEN** 主 Agent 的决策仍可处理，不因 child 容量满而被拒绝。

#### Scenario: The primary reserve is exhausted
- **WHEN** child 请求已占满其阈值，主 Agent 也占满保留的有限容量
- **THEN** 下一个主 Agent 权限请求及时得到不授权结果，已准入请求与控制命令仍可处理。

#### Scenario: Sideband capacity is full
- **WHEN** 权限 Sideband 已达到并发上限，另一个 Auto 裁决到达
- **THEN** 新裁决得到 typed overload，不等待已有 provider；已准入 Sideband 可各自完成，主会话沿现有不可用后续路径处理，child 不打开人工提示。

#### Scenario: Cancellation releases capacity
- **WHEN** 一个在途权限请求的调用方取消并被 actor 丢弃，或已准入 Sideband 结束
- **THEN** 对应资源槽释放，后续请求可准入。
