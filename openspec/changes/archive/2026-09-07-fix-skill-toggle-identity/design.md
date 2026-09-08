## Evidence
Pager ToggleSelectedSkill 发送 skill.name；Shell grow/skills/toggle 按 name 验证并保存 disabled，之后对所有同名项重算 enabled。agent discovery 同样按 name 标记。merge_skills_with_plugins 保留插件限定条目；SkillInfo::dedup_key 已用于合并，提供恰当的既有身份。

## Decision
四处改用 dedup_key：UI 发送、Shell 校验、Shell 结果标记、agent 发现标记。配置说明明确 native name 与 plugin:name。不把 scope 展示前缀混入持久配置身份；不用格式化显示名作 key。

## Verification
真实临时项目与插件目录发现回归：同名原生及插件共存，限定配置只禁用插件、裸配置只禁用原生；既有技能测试和涉及包编译验证。测试不写真实用户配置。
