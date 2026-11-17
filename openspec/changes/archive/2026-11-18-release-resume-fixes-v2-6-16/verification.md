# Verification

## 本地候选

- GitHub 最新公开正式版为 v2.6.15，远端 v2.6.16 引用为空。选用下一补丁版本 v2.6.16，workspace 版本、锁文件、changelog 一致。
- 固定 Rust 1.93.1。全仓 rustfmt、默认 CLI build、默认 CLI check、workspace 全 targets check 和 `release-dist` profile/feature 的 `-D warnings` check 均通过。全 targets check 中既有测试专属警告仍可见。
- 最终候选在隔离 Leader 中分别通过 Workflow 与 scheduler 双 ACP 客户端对照。Pager 最终定向验证 31 项通过、1 项原手动基准忽略，Shell descendant recovery 11 项通过。测试包括 1 MiB 回退、前置终态、分页乱序、子 TODO 重连和不同任务的 scheduler 通知乱序。
- 一个 GPT-6 Astra xhigh 代理做四轮独立源码复审。逐轮修正后，末轮未发现可明确复现的新 P1/P2。复核了原 resident actor 身份、分页任务归属与终态、子 TODO 重连和 scheduler 版本仲裁。
- 最终本机全库复验为 Pager 7,746 passed、15 ignored，Shell 4,881 passed、14 ignored。Shell 一次并发全库运行的配置目录监听用例失败，单项复跑和随后完整复验均通过。使用与 CI 一致的 `RUST_MIN_STACK=16777216`。

## 固定源码的正式门禁

- 发布源码为 `2327036c8a9a162a636df781991c3e42ba24d7f5`，annotated tag 对象为 `b9abf8321cb504e062094cc5393bb2d514d3c9f5`。clean-tree `bash scripts/validate-release.sh v2.6.16` 通过，main 与 tag 原子推送后复核一致。
- [正式 run 44135869400](https://github.com/LordCasser/grow/actions/runs/44135869400) 第二次 attempt 最终 success，20 个 job 全部成功。十平台包、三系统原生协调与导出、Windows 存储、Core 和 OpenSpec 门禁均保留。第一次 attempt 的 macOS 原生 job 排队后因 hosted arm64 runner 容量不足取消，发布被阻止；其余成功结果沿用，随后在同一源码上重跑该 job 并通过。该 job 的独立进程回归也已通过。
- Core job `134225381925` 的原始日志中，16 个无过滤测试组共 19,029 passed、0 failed、40 ignored。其余 Settings 集成、真实 ACP 和已登记 Pager PTY 等精确选组也全部成功。
- main/tag 推送触发的重复协调、Core、Windows 存储运行被取消以释放 runner。它们不作为通过证据。正式 run 内的对应门禁均通过。

## 公开状态

- [公开 release](https://github.com/LordCasser/grow/releases/tag/v2.6.16) 在 `2026-11-17T19:32:21Z` 发布，API 为 draft=false、prerelease=false。发布步骤 `134308465801` 成功。
- 独立验证脚本退出 0，重新核对 run 的固定 SHA、最新 attempt 的 20 个成功 job、annotated tag 指向、latest=v2.6.16，以及精确 11 项资产。十平台包清单完整有序，每项 API digest 与 `SHA256SUMS` 一致。
- 单独下载 macOS aarch64 包，SHA256 为 `695ed71b4ac71689c35d38df0ff285d9b1a03eb6e72caaa43a73a3f0e805dc4f`，与清单/API 一致。包内只有一个可执行 grow 文件；`codesign --verify --strict` 通过。隔离 HOME/GROW_HOME 和 cwd 下执行 `--version` 返回 `grow 2.6.16 (2327036)`，`--help` 正常。
- `gh attestation verify` 对下载包验证通过，绑定 `LordCasser/grow/.github/workflows/build-one.yml` 和完整 source digest `2327036c8a9a162a636df781991c3e42ba24d7f5`。
- 清理本轮新增的 170 个 Cargo incremental 目录，共 33.17 GiB，并清理了双客户端测试的隔离目录及公开包验证临时目录。
