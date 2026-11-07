# Why

用户希望通过 slash 面板控制当前主 Agent 自行调派子 Agent 时的模型范围：默认不选，只继承主模型；选入模型后允许 Agent 按任务自行选择模型和该模型支持的 effort。模型池不配置 effort。用户特别要求先审方案，同意后才实施，重点是主 Agent 如何准确感知变更。

现有 Task 描述列出整个 selectable catalog，并要求只有用户明确点名才填 model；TaskModelValidator、Shell fresh/resume、set_subagent_model 使用 catalog 可选性。单改提示词无法实现用户要求的授权范围，单更新UI也不能保证模型知情。

# What Changes

提议新增 `/subagent-models`，复用 Grow ModalWindow、搜索和勾选样式，为当前主会话维护一份可恢复的模型池。默认空池，effective allowlist 为当前主模型加显式选中的可用模型；effort 始终由 Agent 在所选模型支持值内决定。通过现有 SessionActor Step 控制准入，原子提交池事实、同版模型提醒与工具投影，派发执行端从同版快照校验。

# Approval boundary

本 change 当前只交付方案和候选 delta，不修改产品代码，不派实施任务。只有用户明确同意此方案后才进行实现。该门槛由用户直接提出，不是另加的审批流程。其余已授权 bug 修复不受这个门槛影响。

# Impact

触及会话 Control 快照及压缩恢复、SessionActor、子任务准入和 Pager slash/panel。模型 catalog、provider 连接和 Model/Effort picker 沿现有所有者；不创建另一个模型管理器。不改变已准入子任务的运行模型，也不改 Workflow Run 已冻结的模型路由。
