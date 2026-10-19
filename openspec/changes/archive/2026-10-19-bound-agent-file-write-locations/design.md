# Design

`agent/prompts/foundation/mandatory-core.md` 是英文提示词单一来源，由 `prompt/template.rs` 的 include_str! 嵌入。PromptContext::render_with_renderer 先渲染 Mandatory Core，再按 primary/subagent 选择 Audience；extend/full 只影响独立角色层。因此把一段文字加入 action_safety 即可覆盖各类新构建的 system head，无需重复写入角色或工具描述。

规则约束 Agent 对写入位置的选择：项目及活动 worktree 是正常工作区，/tmp 和平台系统临时目录用于临时产物。任务明确需要配置用户指定的软件、安装依赖或写到指定交付位置时，可以有必要的外部写入，但该必要性必须对应实际目标位置；不能把一般工具权限或任意绝对路径当作写入依据。保留现有破坏性操作与授权规则，不要求每次合规写入再次询问。

这是模型指导，不宣称操作系统强制隔离，也不禁止外部只读调查。命令附带的写入同样属于规则范围；工具正常使用系统临时目录不因 Windows 缺少 /tmp 而被误判。已持久化的稳定 system head 不重写，二进制须重新构建以嵌入新文本。

# Validation

人工核对五类边界：正常项目修改、临时脚本、用户指定外部目标、任务必需的外部配置、仅因方便而写 home。核对共享装配入口及已有 composition 覆盖；确认模板控制标记不变，执行 diff whitespace 与全量 strict OpenSpec 检查。纯提示词文本变动不新增逐字断言测试，也不启动全工作区 Rust 重编译。
