## Evidence
MvpAgent ACP ext_method 将 grow/skills/* 路由到 skills::handle。reset 与 config 使用 from_str(...).unwrap_or(CwdParams { cwd:None })；reset 紧接 update_config 清空 SkillsConfig。其他技能接口直接传播解析错误。

## Decision
使用现有 super::parse_params；CwdParams 自定义 Deserialize 先要求 JSON 对象，再解析可选 cwd，避免 derive 默许数组表示。未知字段沿用忽略规则。拒绝 null、数组与非字符串 cwd；允许空对象、字符串 cwd 和 cwd:null。

## Validation boundary
回归验证生产使用的参数解析器与 CwdParams 的组合，并静态核对 ? 位于写盘之前。旧实现的问题由明确的错误吞掉分支证明；不运行旧无效 reset 去写真实配置，不声称完成 ACP 写盘端到端复现。
