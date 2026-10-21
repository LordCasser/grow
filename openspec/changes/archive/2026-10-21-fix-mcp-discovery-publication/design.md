## 不变量
发现成功是完整目录；注册与搜索快照必须属于当前会话 bridge。远端发现不占用 Agent 切换发布锁。旧 catalog generation 的结果不得发布。禁用、app-only、子代理资格仍按当前规则过滤，诊断不授予执行权限。
## 方案
MCP session 持有一个共享的、受异步 mutex 保护的发布目标 ToolBridge。后台只捕获这个稳定目标；在远程 owned/shared 发现全部完成后才取锁，读取当前目标并完成本地注册与快照。Agent 替换在同一锁下更新目标并恢复已观察目录。共享客户端在发现阶段读取继承 authority pool，发布时按当前 Agent 的更窄资格过滤，允许发现过程中切换继承范围；发现失败保留既有完整目录，成功空目录和资格撤销仍清理。当前 source helper 与 background 资格规则一致；不引入重连、第二套目录或按工具重新执行。
共享 list_all_tools 收集分页，保留顺序，错误整体失败，重复 cursor 拒绝。doctor 统计完整原始目录；搜索快照仍从实际可调用 bridge 生成，不能直接用远端目录绕过资格过滤。
未找到错误仅说明当前注册事实及可用 catalog 线索，不把通用 ToolError 当作服务断线证据。
## 验证
本地 MCP fixture 分页 112 个工具、空/后页错误/cursor 循环；可控延迟 discovery 与 Agent 替换交错，核对旧 bridge 不污染、当前 bridge/snapshot 一致、工具可执行、取消和旧 generation 不发布。保留已有 retirement、禁用和资格回归。
