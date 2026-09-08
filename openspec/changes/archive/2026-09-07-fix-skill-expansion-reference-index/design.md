## Decision
与 skill_blocks 同步收集 SkillRef，移除末尾从全部 parsed_skills 重建索引的逻辑。目录条目消失或正文读取失败均不产生引用；全部失败仍 None。保留成功顺序、参数替换与下层去重。

## Limits
不改变原始用户文本、失败日志或上游诊断事件的统计，本轮仅修正模型提示索引。
