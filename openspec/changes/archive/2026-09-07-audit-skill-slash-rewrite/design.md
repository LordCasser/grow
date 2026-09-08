## Evidence
- slash_commands.rs 的 SkillSlashRewrite 是 pub(crate)，没有 serde 或配置接入。
- 生产入口 admission.rs 与 slash_exec.rs 均固定传 RewriteToRun。
- resolve 的参数名为 _skill_rewrite，函数体不读取它。
- Resolve 命中技能后返回原始 prompt_blocks 和 parsed_skills；admission 另行构造 skill_information，interjection 共用展开函数。
- Passthrough 仅在测试构造。resolve_passthrough_preserves_original_blocks 证明原文保留，但不证明两值有不同语义。
- 全仓检索 SkillSlashRewrite / skill_slash_rewrite 未找到其他生产分支或外部配置。

## Decision
不恢复已脱离架构的 run 前缀或旧 Skill 工具路线。仅改正注释并登记删除候选，遵守用户删除前确认要求。

## Removal boundary
候选仅包括枚举、resolve 的闲置参数、两个生产参数构造和测试传参。保留技能解析、原文保留、正文展开、out-of-band 模型工作拒绝和回归场景。与 R6 旧 Skill 工具协议分别审批。
