# Verification

源码基线 83d76a9d；固定 Rust/Cargo 1.93.1，独立 HOME/GROW_HOME 和本轮专用 target。源码、命令和原始日志见 evidence，不把 Cargo 锁等待计为性能数据。

- 全 workspace/all-targets Clippy -D warnings、独立默认 cli Clippy -D warnings、cli check/build/link 均通过；JSON compiler diagnostics 及非 JSON 构建输出 warning/error 为零。fmt check、diff check 通过。检查范围是本机 macOS arm64、锁定的默认 feature 图，不声称覆盖其他平台和所有 feature 组合，也不把非固定 Homebrew 1.99.1 当作验收工具链。
- 12 个受影响库最终 17,421 passed、0 failed、36 ignored，详见逐库记录。Tools/Diagnostics 内部的并发子进程测试不重复计数。既有 ignored 测试未计为通过。
- 初次组合命令退出 101：前十库通过，Workspace 的 3 项 home 项目分类测试失败。原因是隔离 HOME 位于 /private/tmp，而既有分类器有意拒绝系统临时目录；没有修改分类规则或测试断言。使用同一已编译、SHA 校验的 Workspace 测试二进制，在 /Users/lordcasser/.local/share/grow-quality-rdjj59je-home 独立 HOME 下重跑全库 874/874 通过；GROW_HOME 仍隔离。最后 WorkspaceTypes 单独 51/51 通过。首次失败日志保留，没有把该命令记为成功。
- 可选 Tokio 配置探针：无 process feature 时 -D warnings 退出 0；启用后故意调用原始 spawn，被 disallowed_methods 拒绝并退出 101。探针 wrapper 校验该预期失败，最终退出 0；未放宽实际调用约束。
- 34 处同步路径规范化、3 处异步路径、7 处锁作用域、两处日志选项和冗余 drop 均逐处复核。十个进程点及一个安全路径包装点用局部 expect 记录原有 ownership/规范化理由；不声称移除了现有直接子进程与任意后代清理的差异，相关债务已登记 backlog。首次严格检查因三个例外标错嵌套 Drop 作用域而失败，已修正到准确 spawn let，未关闭 unfulfilled_lint_expectations。

源码先提交为干净状态，随后更新 release-dist 候选并跑现代 PTY/取消回归。release、归档和清理证据将在完成后补齐；本阶段不提前声称这些已完成。专用 target 约 13 GiB，暂供候选构建复用。
