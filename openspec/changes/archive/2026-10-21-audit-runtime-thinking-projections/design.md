## Scope

以已归档 model-sampling 和 session-timeline 契约为基线，核对 Responses/Chat/Messages 的 AgentMessage、SkillLoad、真实工具历史，聚焦与 reasoning 拒绝关联的 wire 形状及恢复后的变化。

## Method

源码分支检查 + 公共 adapter 最小探针。探针明确断言当前观察到的缺陷形状，仅作为审计证据保存；不是要求产品继续维持缺陷的回归。使用临时 Cargo integration target 编译执行后移除该目标，保留 change 内源文件。

区分确定的本地协议形状、生产可达路径与需要真实 provider 才能确认的拒绝表现。未发现生产可达证据的构造边界不能称为线上已复现。不会为审计修复产品代码。
