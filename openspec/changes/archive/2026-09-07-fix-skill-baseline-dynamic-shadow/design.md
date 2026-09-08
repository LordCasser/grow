## Evidence
take_pending/slash_skills/listing_snapshot 均按 discovered 优先合并。update_startup_baseline 只替换 startup_skills；动态副本仍遮蔽相同规范路径的新内容。条件技能激活也调用 add_discovered，故已激活技能重读同样需要处理。

## Decision
完整 incoming baseline 的规范路径集决定接管范围，包含尚未触发的条件技能。更新前移除重叠的 discovered_skills 和 discovered_canonical_paths，再按已有条件激活规则划分 baseline。非重叠动态技能及其同名优先级保留。移除重叠副本本身也要求协调，避免 baseline 恰好未变却留下旧投影。基线之外的动态技能不从本次扫描缺失推断删除。

## Validation
真实临时 SKILL.md 路径，先动态发现再停用/改描述 baseline，验证 slash/runtime/reminder 最新值并保留另一路径动态技能；检查新条件门控不会被旧动态副本绕过。相关生命周期测试与完整 Cargo 构建通过后归档。
