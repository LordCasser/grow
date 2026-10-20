# 发布准备验证

## 已确认的候选

修复提交 `07437efe` 保留本会话源码、常驻回归和九项修复/审计归档。最近相关联合回归 53 passed、0 failed、0 ignored（详见各修复记录）；v2.4.2 在同 SHA 官方 hosted 门禁中重新执行全量 core、真实进程/PTY 和平台专项验证，本地不重复执行未改变语义的版本修改。

workspace 2.4.1 → 2.4.2；Cargo.lock 仅 33 个无 registry source 的本地 workspace 包版本变化，逐项比较确认无依赖/校验和变化。`cargo metadata --locked --no-deps` 确认 cli=2.4.2。新增说明与索引包含全部待发布修复，未合入其他分支。

12 个生产/测试 Rust 文件的 rustfmt 检查通过；首次检查发现原有性能测试格式未规范，已在已修改文件中仅格式化。`git diff --check` 通过。发布脚本测试 13 项执行：12 通过、1 依赖可选环境跳过；未将 skip 计为通过。归档前当前 OpenSpec strict 17 passed、历史 657 passed。

## 后续执行边界

准备归档与版本提交后，才创建指向清洁 HEAD 的 annotated v2.4.2，运行 `scripts/validate-release.sh`，原子推送 main/tag，并从 main dispatch 官方 release.yml。这里尚不宣称 tag、托管门禁或公开资产已完成，实际结果随后追加。

## 首轮候选与发布门禁修复

候选 `85eb50ab` 的 annotated tag 与静态 preflight 通过，已原子推送 main/tag。官方 run 39401343936 在 Python 3.12 SIGTERM 夹具导入时失败；未发布 Release，已取消全部作业。独立修复见 fix-runner-sigterm-fixture-import，后续候选重新校验全部门禁。

## 第二轮候选与编译阶段隔离修复

候选 f264f01d 的 Python 夹具门禁通过，但 macOS/Windows 导出精确用例在 enumerate 再次触发 Cargo 编译而超时，尚未执行产品断言。独立修复 run-exact-tests-from-built-artifact 让后两阶段固定执行已编译 libtest，并保留 package cwd。旧 run 39402132260 已 completed/cancelled，Release 404。候选 annotated tag object 33ed09435095fd685a2322fb890ce4b7b12b24e6 仅在此确认后以 expected-object lease 替换。


## 最终候选与公开发布

发布源码为 `ecb0f9cc6b48d9114432eea6d3ff870302ea9abf`，annotated `v2.4.2` 已推送，远端 peeled tag 与该 SHA 一致。官方 [run 39405009149](https://github.com/LordCasser/grow/actions/runs/39405009149) completed/success，共 20 个作业全部 success，0 failure、0 skipped、0 cancelled；详见 [工作流结果](workflow-verification.json)。此前两个未发布候选的失败未计入最终通过结果。

同一发布 SHA 的门禁完成：Core library/CLI/真实会话恢复/PTY，Ubuntu、macOS、Windows 原生协调与生产导出，Windows session storage，OpenSpec，以及十个平台发行构建与适用的原生冒烟。Core 日志中 11 个完整 library 测试组共 16,488 passed、0 failed、28 ignored；保留原有 ignored，不计作通过。另有 3 个真实会话恢复用例及 27 个精确选择的 PTY/恢复用例全部实际执行并通过。

[公开 v2.4.2](https://github.com/LordCasser/grow/releases/tag/v2.4.2) 于 2026-10-20 11:14:18 UTC 发布，Release API 确认 non-draft、non-prerelease，latest 指向 v2.4.2。公开资产严格为十个平台 tar.gz 与 SHA256SUMS，共 11 个、全部 uploaded 且非空；清单中的十个摘要逐项与 GitHub 资产 SHA256 digest 一致，见 [资产验证](asset-verification.json)。

另从公开 Release 下载 macOS ARM64 包，实际文件 SHA256 匹配；tar 中仅有可执行普通文件 grow，`codesign --verify --strict` 成功。临时解包的二进制在隔离 GROW_HOME 下输出 `grow 2.4.2 (ecb0f9c)`，版本和源码身份匹配。首次本机验证带 `[alpha]` 后缀，核对 update::channel_label 确认它来自已有 version.json 的 stable pointer；隔离配置后完成精确身份验证。该显示未改变公开 Release 的 non-prerelease 状态。

发布结果入库前再次执行 strict 校验：当前规范/变更 16 passed、0 failed；历史归档 660 passed、0 failed；`git diff --check` 通过。
