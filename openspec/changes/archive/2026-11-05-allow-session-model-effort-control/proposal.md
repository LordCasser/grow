## Why

当前 subagent resume 会丢弃显式 model/effort，模型也没有通过既有会话控制边界调整自身采样配置的原生工具。用户需要在继续任务时选择模型和推理强度，同时保留用户切换主会话时的权限、Step、持久化和采样边界。

## What Changes

- resume 接收可选 model 和 effort，分别覆盖来源选择；未提供的字段保留来源实际值，包含未设置的 effort。显式模型按当前可选目录解析，effort 对最终模型校验。
- 新增 Grow 原生 `set_session_model` 工具，仅调整调用它的当前会话。支持 model、effort 或两者；返回接纳结果，在现有闭合 Step 边界应用，不同步等待自身 Step 结束。
- 复用用户入口的模型选择限制、Workflow frozen route、effort 校验、actor 排队、durable route transition、native continuation 与控制失败边界。
- 按当前 software-engineering skill 对相关实现链路专项 review，修复阻塞本次验收的问题，其他有证据的债务单独记录。
- **BREAKING**：resume 显式参数不再被忽略；非法或不支持的选择返回失败。

## Capabilities

### New Capabilities

无；扩展已有会话与模型采样能力。

### Modified Capabilities

- `model-sampling`：resume 显式覆盖与当前会话模型工具的采样控制语义。
- `session-timeline`：resume 保留来源事实并为派生 child 记录最终选择，原身份挂起恢复保持原契约。
- `tool-authorization`：模型工具绑定调用会话，复用模型选择与工具执行授权边界。

## Impact

涉及 common/tool-types 的 Task schema、tools 的 Task 与新工具适配、shell 的 subagent resume 和模型控制路径、相关测试及用户/开发者说明。不增加 provider、配置格式、独立模型状态库或任意跨 session 控制接口。保留开始任务时已有的未提交工作；专项清查不构成全仓库重构授权。
