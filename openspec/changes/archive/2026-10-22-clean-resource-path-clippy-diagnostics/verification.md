# Verification

源码基线 83d76a9d；固定 Rust/Cargo 1.93.1，独立 HOME/GROW_HOME 和本轮专用 target。最终命令与校验摘要见 evidence，既有质量扫描日志保留；发行/PTY 原始采集仅在工作区外临时保留，最终结论不依赖临时目录存在。不把 Cargo 锁等待计为性能数据。

- 全 workspace/all-targets Clippy -D warnings、独立默认 cli Clippy -D warnings、cli check/build/link 均通过；JSON compiler diagnostics 及非 JSON 构建输出 warning/error 为零。fmt check、diff check 通过。检查范围是本机 macOS arm64、锁定的默认 feature 图，不声称覆盖其他平台和所有 feature 组合，也不把非固定 Homebrew 1.99.1 当作验收工具链。
- 12 个受影响库最终 17,421 passed、0 failed、36 ignored，详见逐库记录。Tools/Diagnostics 内部的并发子进程测试不重复计数。既有 ignored 测试未计为通过。
- 初次组合命令退出 101：前十库通过，Workspace 的 3 项 home 项目分类测试失败。原因是隔离 HOME 位于 /private/tmp，而既有分类器有意拒绝系统临时目录；没有修改分类规则或测试断言。使用同一已编译、SHA 校验的 Workspace 测试二进制，在 /Users/lordcasser/.local/share/grow-quality-rdjj59je-home 独立 HOME 下重跑全库 874/874 通过；GROW_HOME 仍隔离。最后 WorkspaceTypes 单独 51/51 通过。首次失败日志保留，没有把该命令记为成功。
- 可选 Tokio 配置探针：无 process feature 时 -D warnings 退出 0；启用后故意调用原始 spawn，被 disallowed_methods 拒绝并退出 101。探针 wrapper 校验该预期失败，最终退出 0；未放宽实际调用约束。
- 34 处同步路径规范化、3 处异步路径、7 处锁作用域、两处日志选项和冗余 drop 均逐处复核。十个进程点及一个安全路径包装点用局部 expect 记录原有 ownership/规范化理由；不声称移除了现有直接子进程与任意后代清理的差异，相关债务已登记 backlog。首次严格检查因三个例外标错嵌套 Drop 作用域而失败，已修正到准确 spawn let，未关闭 unfulfilled_lint_expectations。

## 发行候选与运行验证

产品源码提交为 `35548386343ba0f0416d3fac6cde2c25c36ea013`，从干净状态构建 release-dist。编译/链接成功、零 warning/error。包装脚本随后因工作区全程干净断言失败而退出 1：构建期间另一个任务清理了 docs/OpenSpec 历史证据。核对 `git diff 35548386` 只有文档改动，编译输入不变；复用成功生成的二进制完成包装。该并发文档任务随后提交 `ab41c8428`，按用户要求保留在本轮 2.5.0 同一提交链中；它没有产品源码变化，本项不重复提交或改写该独立提交。失败归因与边界复核结论记入本记录；原始包装日志与临时脚本只在工作区外临时保存，不把 wrapper 的 exit 1 说成 exit 0。

新候选为 `grow 2.5.0 (3554838)`，arm64、102,430,896 bytes，SHA256 `bbf18e41ab1e81ec4b8fbcb055854bc2d048cd174ae472e5312fd6a1c4856fd8`；位置见 evidence/final-candidate-manifest.json。strip 后 ad-hoc 签名与 strict verify 通过，动态依赖仅 macOS 系统库，--help/--version 正常。pinned ripgrep 的归档及二进制 SHA 均按上一候选 manifest 校验，再嵌入新候选。

四项 exact 测试最终各 1 passed/0 failed/0 ignored：现代 512/2048 轮完整恢复、512 轮慢终端输出、现代 loading 中 Ctrl+D 退出及再次恢复、子会话 orphan 对账。沿用既有完整恢复/ready 边界，验证历史进度/顺序、末屏和 ready 输入、持久化与 child terminal 关系。首轮正常恢复用例本体通过，但本地采集 helper 对 nocapture 交错输出的名字行解析失败；改为同一 exe 枚举唯一精确名字、--exact 执行和 1/0/0 summary 校验后重跑通过。未改仓库精确测试 runner 或产品测试断言，逐次输出与 trace 只在工作区外临时保留。测量仅作回归依据，没有严格配对性能对照，不宣称 Clippy/Box 清理提高 resume 速度。

## 清理

已删除本轮 target/rust-quality-2-5-0，清理前 allocated 20,638,973,952 bytes（约 19.2 GiB）；两个隔离 HOME 与 GROW_HOME 已删除。结束审计无本轮测试进程残留，无需终止进程；保留更新后的本机候选及 manifest，不动 target/debug、target/release-dist 或共享 Cargo/工具链缓存。原始采集目录删除，不含凭据的原始日志包仅在工作区外临时保留，不包含编译产物。源码哈希证明最终产品代码与已验证提交一致；后续仅本项文档/证据归档。

归档后严格校验：现行 16/16、已归档 699/699 通过；归档任务已完成并补齐全部 tasks。
