## Evidence
ToolRegistryBuilder::new 在 tools/src/registry/types.rs 显式注册内置工具与 SkillDiscoveryReminder，但没有 Skill tool。全 crates 检索 SkillInput/SkillOutput/SkillToolImpl：只有协议类型、枚举包装、匹配分支；未找到执行实现。grow_build/mod.rs 没有 skill 模块，skill.rs 的“新实现位于 grow_build/skill”注释已过时。

ToolPack 公开扩展入口存在，但仓库内仅定义，未找到注册调用。因此结论仅限当前仓库，不证明仓库外 Rust 集成不存在。

旧 IO 分支仍位于 permission、authorization、preparation、normalization、task_completion、ToolOutput 格式化及 ACP 转换。它们可处理构造/反序列化的旧值，但不能证明内置工具可被调用。删除涉及已序列化 ToolInput/ToolOutput 的兼容面，应在用户确认范围后统一移除，不能只删结构体留下匹配分支。

## Preserved functions
skill.rs 的 build_skill_message 被 agent 预加载调用；build_skill_block 被 slash 展开调用；load_skill_with_body 被 agent_rebuild 与 workflow 使用。其他格式化、链接和参数替换函数不在删除范围。同文件不能整体删除。ToolKind::Skill 的分类与模板标记另有引用，也不并入 R6。

## Outcome
R6 候选：SkillInput/SkillOutput、ToolInput/ToolOutput 对应枚举分支及只服务这些值的处理分支。没有删除任何代码；后续执行前重新核对引用和外部扩展。

## Validation limits
静态注册和引用审计，不声称运行时外部 ToolPack 不存在，未新增镜像实现的测试，未更改生产代码。
