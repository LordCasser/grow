# Grow 2.5.1 发布验证

## 发布准备

2026-10-25（Asia/Shanghai）。当前组合工作树含五项已归档行为变更，纳入 2.5.1。对照主规范、实现调用链和已有回归记录，未扩展未完成的 MCP 交互提案。

- Rust/Cargo 固定 1.93.1（显式将 rustup 代理置于 PATH 首位）；`cargo metadata --locked --no-deps --format-version 1` 通过，CLI/workspace 为 2.5.1，33 个继承版本的本地 lock 包同步。
- `cargo fmt --all -- --check`、`git diff --check` 通过。
- `python3 -m unittest discover -s scripts/tests -p 'test_release*.py'`：13 项，12 passed / 1 skipped；未把平台跳过计为通过。
- `openspec validate --all --strict --no-interactive`：17 passed；`openspec validate --archived --strict --no-interactive`：709 passed。补足 busy effort 归档后组合状态的校验。
- 复核 Goal completion_report 的输入/authority/交付门槛、Control 持久化 ACK 与失败回滚、typed history 和 live/replay 展示；上下文恢复保留 raw history、真实 overflow 和追加差值；技能发布使用同一 projected_tokens；忙碌 effort 只结算匹配 intent；/btw 遮挡、链接与历史查看均有现有回归。

本地已有五项专项验证不冒充最终发布 SHA 的全平台验证。官方工作流需在最终标签提交上执行全部必需回归和十平台构建，全部通过才发布；发布结果与公开包验证稍后补记。

## 固定发布身份与本机检查

Source：`a58b5dd7e8c75c944b438593b1f2eca3b99badf4`；以 annotated `v2.5.1` 推送，main 与标签原子发布。`scripts/validate-release.sh v2.5.1` 在干净工作树通过，官方流程为 https://github.com/LordCasser/grow/actions/runs/40196888908 ，全部平台组开启、publish=true。

在该 source 上以 Rust/Cargo 1.93.1、CARGO_BUILD_JOBS=2、CARGO_INCREMENTAL=0 执行 `cargo check --locked -p cli --bin grow`（1m16s）与 `cargo check --locked --workspace --all-targets`（9m29s）均通过。编译诊断日志无 warning/error；复用现有 target，只执行 check，未创建专用 Cargo 编译树或新增 incremental cache。

## 正式发布

[Grow v2.5.1](https://github.com/LordCasser/grow/releases/tag/v2.5.1) 于 2026-10-25 22:24:29（Asia/Shanghai）公开，Latest=true，非 draft/prerelease，release notes 与标签文件一致。annotated tag 对象为 `3ad53badad4bf38fc116e947b41cbff4b21f8024`，source 保持上述 a58b5dd7，发布后未移动标签。

[官方 run 40196888908](https://github.com/LordCasser/grow/actions/runs/40196888908) 的 20 个 job 全部 success，无失败、跳过或取消的必需门禁。覆盖十个平台构建、Core/Settings/真实 CLI sampling failure recovery/选定 PTY、三平台 native coordination/export、Windows storage 与 OpenSpec。`ci-results.json` 保存精简结果与时间。相同 source 的 push Core、coordination、Windows storage、OpenSpec 也全部 success。

核对 Core 原始日志，`minimal_does_not_scroll_unpainted_btw_geometry` 在完整 Pager library suite 中实际通过，补足早期 /btw 专项停在该测试的验证边界；没有跳过或放宽它。

## 公开资产与本机冒烟

从公开 release 重新下载全部十个 archive 与 SHA256SUMS，精确核对 11 项资产、API size/digest、本地 SHA256、清单覆盖与无重复项。每个 archive 只有一个普通可执行文件（Unix grow / Windows grow.exe），并核对 ELF/Mach-O/PE 与目标架构。十项均通过 `gh attestation verify`，限定官方 build-one.yml signer 和完整 source digest。最终摘要见 `published-assets.json`。首次批次的 macOS ARM64 attestation 命令退出失败（初次包装未保留 stderr，不据此推断原因）；单独重试及随后完整验证均成功，未降低验证条件。

macOS ARM64 公开包在独立 HOME/GROW_HOME、仅继承 PATH 等基础变量的环境通过 codesign --verify --strict、精确版本 `grow 2.5.1 (a58b5dd)`、--help 和完整 `scripts/test_local_coordination.py` 真实进程回归，otool 仅列系统动态库。协调回归使用 loopback 受控模型，不读取个人模型配置或调用付费 provider。摘要见 `native-smoke.json`。

保留候选 `target/local-release-2.5.1/grow` 与 manifest，100921136 bytes，SHA256 `e350352e8939f9bb4143b9931864bdfbddefb8237bd584bf8fca6028534a5d60`。未替换用户 PATH 中安装的程序。跨编译资产的架构/签名验证不冒充目标平台原生运行；本机未做交互终端人工视觉验收。

## 归档与清理

发布记录以 skip_specs 归档；归档时最后一项为归档/清理/最终提交本身（3/4），产品与公开资产验证已完成。专用下载/解包/隔离 HOME/GROW_HOME/原始日志和辅助脚本已删除（普通文件逻辑大小 562500640 bytes）；候选进程剩余 0。没有新增专用 Cargo 编译树，未删除共享 target 或其他任务缓存。保留候选重新核对版本与 SHA256。

归档后最终校验：16 项当前规范/提案与 710 项归档均通过，git diff --check 通过。保留候选在重新创建的隔离 HOME/GROW_HOME 中报告精确版本 `grow 2.5.1 (a58b5dd)`，SHA256 未变；临时环境随即删除。
