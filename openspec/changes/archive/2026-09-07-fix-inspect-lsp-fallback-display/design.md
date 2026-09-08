## Context
inspect_report 已计算 project_trusted；list_lsp_servers 却二次计算并总是让项目参与覆盖。LspServerEntry 有 source 和 untrusted，列表可容纳同名不同来源，无需添加第二套协议字段。
## Goals / Non-Goals
让允许来源和被禁用项目项都可见，保持已信任项目覆盖。此次只处理项目信任下的配置来源展示，不将列表声称为正在运行的进程或插件启用状态清单。
## Decisions
传入现有报告信任快照，sourced loader 只合并允许的项目来源；未信任时另读取项目文件作为显式 untrusted 诊断项。同名允许项排在禁用项之前，按名称排序保证稳定；JSON 和终端共用同一条目。
## Risks / Trade-offs
同名条目会出现两行，用来源和 untrusted 区分；消费者不能只按名称覆盖列表。读取两类配置仍为观察动作，不承诺文件系统快照原子性。
