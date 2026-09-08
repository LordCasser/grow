## Evidence
Workflow tracker 冻结发现结果时调用 load_skill_with_body，agent rebuild 使用冻结 SkillInfo。生产者将空正文转换为 None，load_skill_content 又过滤 Some 空值，因此冻结空文件仍然依赖磁盘。

## Decision
保留 Option 类型，Some 总是权威正文；成功读取后直接保存 Some(body)。None 才允许读取磁盘，synthetic path 的 None 继续报错。测试覆盖真实文件冻结后修改和删除、显式空 synthetic body、未加载文件正常读取。

## Scope
不迁移历史快照，也不改变正文大小策略或引入新缓存。load_skill_with_body 仍作为显式磁盘加载入口。

## Consumer reconciliation
agent::prompt::skills::resolve_preloaded_skills 同样把空值转换为 None，本次一起保留 Some。format_skill_for_injection 继续忽略空正文，因此空值快照不会增加 agent 提示词。Shell synthetic-path 错误日志分支不负责验证正文；None 仍会进入原错误路径。
