## Evidence
- tools discovery::coerce_tool_list 解析并复制到 SkillInfo.allowed_tools，workspace-types rpc/skills 也有序列化字段。
- Pager extensions_modal 技能详情将 allowed_tools 显示为 tools 文本，是实际展示消费者。
- Skill build_skill_message/build_skill_block 不使用 allowed_tools；slash_commands 调用处未读取此字段。
- 全仓 Rust 字段引用未找到权限执行消费者，workspace permission 与 shell actor/tool/subagent 中实际使用的 CLI disallowed_tools 是另一条能力边界。

## Conclusion
该字段当前是可展示元数据，不能据此声称限制工具访问。保留功能，说明其边界；是否升级为执行约束需先设计调用生命周期、合并规则与权限上限。错误类型当前丢字段是元数据质量问题，不能描述为已证实的权限绕过。
