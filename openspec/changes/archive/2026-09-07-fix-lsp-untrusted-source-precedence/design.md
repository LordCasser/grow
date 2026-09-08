## Context
`load_servers_with_plugins_sourced` 当前 project > user > plugin；后置 `filter_project_lsp_when_untrusted` 只能删除最终 Project 项，无法恢复被遮蔽的可信配置。Workspace 持有 project_lsp_trusted，Shell 可查询 project_scope_allowed，inspect 不启动进程。
## Goals / Non-Goals
在来源参与覆盖前实施既有许可。保留后置过滤以应对加载期间信任状态变化。不扩大 Folder Trust 权限，也不合并管理器或增加状态。
## Decisions
为既有 loader 添加显式 include_project 参数。Workspace 和 Shell 传真实 trust verdict；inspect 传 true 以保持展示被禁用项目配置的已有行为，再按信任标记。使用真实项目与插件 JSON 文件和 inline 插件值复现遮蔽问题，避免修改全局 GROW_HOME。
## Risks / Trade-offs
若信任在读取中撤销，后置过滤仍会阻止 Project 项，低优先级来源可能暂时需要下一次加载才恢复；不将该瞬态问题混入此修复。inspect 是诊断视图，显示被禁用覆盖项不代表会启动。
