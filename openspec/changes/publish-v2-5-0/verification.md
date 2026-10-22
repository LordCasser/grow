# 当前发布状态：未公开，等待标签决策

发布准备提交为 `ac4ebb7630cf3c9b623af9d40678021c23faf1e4`，annotated tag 对象 `9015ba6cb3fce6704657cf853dc920ba52454e87`，远端 v2.5.0 尚未移动。notes 已整理，含 Timeline v27 不迁移旧会话的升级边界；静态发布脚本、发行工具测试与 OpenSpec 严格校验通过。main 与 tag 已原子推送；使用现有 release.yml，四平台组全部开启、publish=true。

[官方发布 run 39770985598](https://github.com/LordCasser/grow/actions/runs/39770985598)：十个平台 archive 构建均成功；三平台原生协调/导出、Windows storage 和 OpenSpec 均成功。Core 首次执行在未改动的 LSP mock diagnostics 用例等待 summary 时超时；同提交 push 的 core library suite 通过。只重试 Core，复用已成功任务，重试也已通过 core/presentation/Settings 和 live session recovery。

同提交 [push Core run 39770985516](https://github.com/LordCasser/grow/actions/runs/39770985516) 首次与一次重试均在 deferred Plan 恢复后审阅场景超时。检查发现夹具把首段历史可见当成输入就绪，可能在产品按契约忽略输入的窗口发送 /view-plan。未继续碰运气重跑，也未修改或绕过门禁；主动取消仍在进行的发布 Core 重试，release 未公开。不能声称最终 CI 成功或完整公开资产已验证。

已独立修正并归档 [Plan resume PTY 就绪同步](../archive/2026-10-22-synchronize-plan-resume-pty-readiness/verification.md)，提交 `e2e6ee7d`。使用该发布 run 的 macOS ARM64 archive，来源证明绑定 build-one.yml 与 source digest ac4ebb7630cf3c9b623af9d40678021c23faf1e4、codesign、隔离环境版本校验通过；修正后的两种 Plan 跨进程恢复均通过。此仅核对原产品和新夹具，不代表已完成十个平台公开下载、SHA256SUMS 或最终发行提交的回归。

后续：需明确未公开 tag 的处理，再以包含修正的精确提交重新通过官方流程。不得复用旧 source 的编译包冒充新 source 的产物，不移动已公开 release，不提前开放恢复输入，不做就绪与历史分页分离。发布 change 保持未完成，不归档成成功发布。
