## Evidence
全 crates 检索 AvailableSkills / has_skill：生产端仅 registry/types.rs 初始化写入、bridge.rs reconciliation 写入；读取在 registry/types.rs 的测试，has_skill 只有定义。桥接层提供泛型资源读取，但未找到生产调用者指定 AvailableSkills。

实际 slash 候选通过 bridge.slash_skills 从 SkillManager 获取；shell/session/slash_commands.rs 过滤 user_invocable && enabled。不能据 AvailableSkills 陈旧推断禁用技能绕过执行检查。

SkillInput/SkillOutput 虽然有旧 Skill tool 注释，仍被 ToolInput/ToolOutput、permission、preparation、normalization 和 ACP 转换引用；本次不列入删除范围。对未找到 grow_build/skill 模块只记为注释不可信，不推断所有外部工具注册不存在。

## Follow-up
AvailableSkills 类型、has_skill 和只维护该副本的生产写入列为 R5，等待用户确认。动态发现合并语义、SkillManager、slash、提示与预加载功能全部保留。删除前重新核对泛型资源消费、外部 Rust 集成和测试迁移。

同路径 metadata 修复仍有实际作用：pending BaselineChange 会清空 announced 并重新渲染提示，description 更新可进入新的系统提醒。对内部 runtime_skills 同步的测试是事实，但不是实际权限绕过的证明。
