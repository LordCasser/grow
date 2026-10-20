# 发布准备验证

## 已确认的候选

修复提交 `07437efe` 保留本会话源码、常驻回归和九项修复/审计归档。最近相关联合回归 53 passed、0 failed、0 ignored（详见各修复记录）；v2.4.2 在同 SHA 官方 hosted 门禁中重新执行全量 core、真实进程/PTY 和平台专项验证，本地不重复执行未改变语义的版本修改。

workspace 2.4.1 → 2.4.2；Cargo.lock 仅 33 个无 registry source 的本地 workspace 包版本变化，逐项比较确认无依赖/校验和变化。`cargo metadata --locked --no-deps` 确认 cli=2.4.2。新增说明与索引包含全部待发布修复，未合入其他分支。

12 个生产/测试 Rust 文件的 rustfmt 检查通过；首次检查发现原有性能测试格式未规范，已在已修改文件中仅格式化。`git diff --check` 通过。发布脚本测试 13 项执行：12 通过、1 依赖可选环境跳过；未将 skip 计为通过。归档前当前 OpenSpec strict 17 passed、历史 657 passed。

## 后续执行边界

准备归档与版本提交后，才创建指向清洁 HEAD 的 annotated v2.4.2，运行 `scripts/validate-release.sh`，原子推送 main/tag，并从 main dispatch 官方 release.yml。这里尚不宣称 tag、托管门禁或公开资产已完成，实际结果随后追加。
