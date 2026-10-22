# Grow 2.5.0 发布验证

[公开 release](https://github.com/LordCasser/grow/releases/tag/v2.5.0) 已发布且为 latest，非 draft / prerelease。release id 为 `420909068`，发布时间为 2026-10-22 23:49:01（Asia/Shanghai）。发布时 notes 与标签内 `crates/codegen/shell/changelogs/2.5.0.md` 一致，包含 Timeline v27 不迁移旧会话的升级边界。

最终 source 为 `8443a4d1648d17b10f993c845cb68d949c55235e`；annotated tag 对象为 `f1e99dff7af3b499f8c8d432ba7e9d745b7c8829`。本机预检通过后，以 expected old tag object 的 force-with-lease 原子推送 main 与用户授权重建的未发布标签；之后未移动。使用现有 release.yml，四个平台组全部开启、publish=true，全部资产重新构建，未把旧 source 的包冒充新提交。

## 最终 CI

[官方发布 run 39797866048](https://github.com/LordCasser/grow/actions/runs/39797866048) 在 attempt 1 成功，head SHA 与 tag 精确匹配，20 个 job 全部成功，无跳过或取消的必需门禁。包括十个平台构建、Core / presentation / Settings / 实际 CLI 恢复 / 所选 PTY、三平台 native coordination/export、Windows storage 与 OpenSpec。两种 Plan 恢复分别实际执行且通过：`plan_approval_restored_after_resume`、`deferred_plan_reviewed_after_resume`。

同提交 push [Core](https://github.com/LordCasser/grow/actions/runs/39797846368)、[协调](https://github.com/LordCasser/grow/actions/runs/39797846608)、[OpenSpec](https://github.com/LordCasser/grow/actions/runs/39797846276) 也全部成功。

## 公开资产与本机冒烟

从公开 release 重新下载全部十个平台 archive 和 SHA256SUMS，精确核对 11 项资产、API size/digest、清单 coverage/no duplicates 与本地 SHA256。所有 archive 只含一个普通可执行文件（Unix grow / Windows grow.exe），无额外路径、链接或设备；逐一核对 ELF / Mach-O / PE 和对应架构。十项均使用 gh attestation verify 核对官方 build-one.yml signer 与完整 source digest，通过。完整最终资产摘要见 [published-assets.json](published-assets.json)，不保存原始下载、日志或构建目录。

macOS ARM64 公开下载包在独立 HOME/GROW_HOME 下通过 codesign --verify --strict、版本 `grow 2.5.0 (8443a4d)`、--help 和完整 scripts/test_local_coordination.py。协调冒烟覆盖独立进程发现、busy/queue/dedup、结构化失败、payload conflict、取消与授权、持久审计与重启、crash receipt、同 identity 跨 peer 隔离等，退出码 0。otool 只列系统动态库。使用本地受控模型，没有个人 provider 配置或付费请求。

保留官方本机候选 `target/local-release-2.5.0/grow` 与 manifest，替换前核对旧候选未被他人改写；新二进制 SHA256 `22fa55199b1c5bd7594b1bf19d5017829b5614b0c74593ab77bbab8df48c07f4`，100921072 bytes。未修改用户 PATH 中已安装的程序。

边界：六个平台的发行 job 执行了原生 smoke；Linux 两个 musl、RISC-V 与 OHOS 沿用既有交叉构建能力，未执行其目标平台原生 smoke，不将架构/签名验证称作运行验证。未做就绪与历史分页分离。

## 失败处置与对抗性审查

初始 source ac4ebb763 的 [发布 run 39770985598](https://github.com/LordCasser/grow/actions/runs/39770985598) 十平台构建成功，但 Core 首次在 LSP mock summary 的限时断言失败。同提交 push library suite 通过；独立债务保留于 backlog，没有通过放宽超时处理。

旧 source 的 [push Core 39770985516](https://github.com/LordCasser/grow/actions/runs/39770985516) 首次与一次重试又在 deferred Plan resume 场景同一步失败。发现夹具把首段历史可见当成输入就绪后，不再碰运气重跑，主动取消旧发布重试。修正以 [独立 change](../2026-10-22-synchronize-plan-resume-pty-readiness/verification.md) 提交 e2e6ee7d，保留原审批、持久化和恰好一次采样断言。本机验证后用户明确允许重建尚未公开的 v2.5.0，再执行上述完整新发布。未公开旧 release、跳过门禁或改变产品恢复输入规则；已公开的最终 tag 不再改写。

清理：发布临时目录（删除前 allocated 为 693456 KiB）已删除，包含全部下载、临时解包、隔离 HOME/GROW_HOME、原始日志及辅助脚本；专用夹具编译树已在独立 change 删除。按测试 binary 路径复查剩余进程为 0。保留上述官方候选与 manifest，最终摘要和隔离环境版本再次核对通过。未删除共享 Cargo 目录或用户会话。

最终归档校验：Totals: 16 passed, 0 failed (16 items)；Totals: 701 passed, 0 failed (701 items)。最终提交只含发布记录、已解决夹具的 CI 结论及 backlog 更新，不改变产品或公开标签。
