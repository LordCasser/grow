## Decision

协调器始终给子 Agent 提供现有 Config 快照。直接使用它解析实际目标模型的阈值、摘要输入模式和工具结果裁剪参数，删去重复的子 Agent 配置视图。Workflow 继续使用其已捕获路由中的阈值。两个入口都通过同一 SessionActor 执行压缩，按各自会话的窗口、权限和 Timeline 工作。
