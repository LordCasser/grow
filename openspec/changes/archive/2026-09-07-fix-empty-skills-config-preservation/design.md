## Evidence
当前未知字段回归使用非空 paths，因此未进入 default 分支。只包含未知键的 skills 会被解析为默认 SkillsConfig，下一次任何设置修改都会 table.remove("skills")。

## Decision
默认分支从 SkillsConfig 默认值的 TOML 序列化键获取当前已知字段，移除这些字段后仅在剩余表为空时删除。现有五个字段都序列化，包括空数组；未知子表完整保留。非默认分支继续使用现有 merge_section。

## Verification
真实临时文件覆盖仅未知字段+无关设置变更、最后一个 disabled 清空且未知子表保留、仅已知字段清空后整段删除。运行持久化回归。
