## Why

发布审查发现 primary audience 中新增的工具条件提示不会到达生产请求：AgentBuilder 调用 PromptContext::render，而该入口用空工具集合渲染稳定 system head。直接用完整 renderer 渲染模板的测试掩盖了这个缺口。稳定 head 也不应随 Agent 工具集改变。

## What Changes

- 将子 Agent 枚举、状态、等待和结果判定的工具相关提示移入已有 session extensions 动态 Agent 层，按真实工具可用性渲染。
- 保留 audience 的职责、协作与验收要求；稳定 head 不引用动态工具。
- 用 PromptContext 生产入口覆盖默认、重命名、缺工具及 Full/Extend 组合，更新 Agent 开发说明。
- 对齐既有 resume 契约的工具说明：继承 transcript，重建 runtime/tools，不复制旧 permit。

## Impact

涉及 agent prompt composition，不改工具接口、权限、Timeline 格式或执行控制。修复同一发布候选中被省略的提示，不增加新的协调协议。
