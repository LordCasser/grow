## Reproduction
旧实现 ignore_paths_apply_to_plugin_skills_before_merge 失败：ignore 指向 plugin 根目录，仍返回 demo:shared 与 demo:sibling。无 ignore 对照先确认两个插件技能实际可发现。

## Validation
agent prompt::skills：92 passed。新回归覆盖插件根目录、单个 SKILL.md、原生同名保留与未忽略 sibling；既有原生同名低优先级回退回归同时通过。

## Limits
验证真实临时目录发现路径，不覆盖未知技能动态加入的忽略策略，不修改插件 registry 启用状态。未执行完整 UI；已安装 grow 未更新。target 约 6.5 GiB，剩余磁盘约 74 GiB。
