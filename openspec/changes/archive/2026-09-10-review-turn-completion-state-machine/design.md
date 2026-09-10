## Method

以 provider 响应事实 → 中性响应 → Surface/工具结果 → Step 控制边界 → Turn terminal → idle/Goal 调度为主线。将已确认实现缺陷、设计缺口、模型能力假设及尚未动态覆盖的组合分别记录。

Atlas 仅执行 project open 和局部查询；查询返回 partial/closure_boundary，因此使用已定位源文件核对实际分支，不将空查询当作不存在调用。探针直接调用现有 sampler 和请求编码器，不复制被审查函数。既有测试二进制只证明其对应编译版本的覆盖；新发现的宿主交叉路径以源码证据说明，不把分层探针称为完整端到端测试。

## Scope

审查不扩展到 Workflow、所有工具实现或整个仓库。明确区分更换 endpoint/backend、修改 reasoning effort、热重载相同模型和 Agent/Behavior 变更。后台等待需要同时审查依赖身份与调度准入，不能只检查终态字符串。
