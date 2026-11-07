# Release candidate design

候选继承当前 2.6.1 之后的已提交和工作区成果。父 architect 负责逐项验收、主规范归档、backlog 状态与最终发布说明，coder 只负责各自行为的实现与验证。当前 HEAD 为 8ab486a6e，main 比 origin/main 领先四次提交；工作区有大量既有修复，不整体回退或覆盖。

版本只更新 workspace version 及由它派生的本地 lock 条目，不升级外部依赖。发布说明只写已验收结果，明确平台/现场限制；不把本地 debug 构建当成正式分发资产。

验证显式使用 rustup 1.93.1 与标准 PATH，CARGO_INCREMENTAL=0、CARGO_BUILD_JOBS=2、RUST_MIN_STACK=16777216。按修复运行定向回归；最终候选执行 fmt、default CLI check、workspace all-targets check、default build、release-dist build、隔离 version/help smoke 和 OpenSpec all/archived。真实入口里程碑使用本次显式 binary，记录 source 与 binary identity。官方完整 CI 和十平台资产由既有 workflow 执行；未执行前不声明通过。

`scripts/validate-release.sh` 要求干净已提交源码和 HEAD 上的 annotated tag。准备阶段可验证相关静态契约与候选内容，但不能为了让该脚本通过就擅自提交所有既有工作或创建共享发布身份。最终说明应准确区分本地候选验证与正式发布前剩余动作。
