## Evidence
reminders/skill_discovery.rs 直接 SKILL.md 和向上发现均从 discovery 原始解析进入 add_discovered。该方法不查询 startup_skills，动态投影又优先，故配置停用可被原始 enabled=true 副本遮蔽。

## Decision
按规范路径检查当前可见 baseline，已包含则跳过（baseline 重载负责更新）。若当前 conditional held 已有该路径，采用 held 的元数据再检查激活状态，避免新解析未带 paths 或配置字段时绕过；真实条件激活仍通过原 add_discovered 路径进入可见集合。不同路径动态发现维持原行为。

## Limits
只保证已知同路径 baseline/conditional 权威，不给配置 ignore 或未进入 baseline 的未知路径虚构全局禁止语义。磁盘内容的主动刷新仍由 baseline reload 完成。
