## Evidence
解析器将 model/effort 复制到 SkillInfo，workspace-types RPC 同样保存字段。全仓字段引用未找到消费技能 model/effort 的采样入口。build_skill_information_for_refs 仅读取正文并替换参数，返回 Option<String>，admission/interjection 消费该提示文本；build_skill_message/block 也未使用这些元数据。采样配置由 session actor 的 get_sampling_config 等路径构造。

## Decision
文档说明当前只保留元数据、不覆盖会话采样。R10 供用户决定是否删除这组未接入字段，执行前重新检查外部 RPC 消费；不以新增多技能覆盖策略代替现有范围。
