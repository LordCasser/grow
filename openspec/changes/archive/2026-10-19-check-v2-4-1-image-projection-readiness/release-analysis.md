# v2.4.1 发布前核对 — 2026-10-19

## 结论

本轮图片占位修复已单独提交到 main 的 `565bbd24`（父提交为独立审计的 `cd98797f`）。该提交的默认 CLI 检查暴露生产导入遗漏，已在本维护 change 用现有 Shell re-export 修正。工作区与 lock 身份一致，但产品版本仍为 2.4.0，2.4.1 发布材料和最终源码的托管平台验收尚未完成；不能将本轮本地通过写成 v2.4.1 可立即发布。

## 源码与交互

- 原修复仅含五个源码/测试文件、开发说明、两个主规范新增 requirement 和一份行为 change 归档；与七项独立审计顺序提交，共享文档按 hunk 分离，没有混入其他任务。
- 模型侧不制造图片投影 assistant 话语；历史精确占位从派生请求/子上下文排除。TUI 使用共用文本块，覆盖 normal、minimal、child 和 deferred/replay；完整占位隐藏，分叉、未完成前缀及 finish 后真实追加恢复原文，`Converted` 操作行保留。原始证据和 canonical projection digest/updates 不变。
- 本轮新增维护修正只将 Pager 的常量引用改为 `shell::sampling::conversation` 已有 re-export，常量内容、解析、配对和展示逻辑均未改变。没有新增 dependency 或放宽 lint。缺陷来自 sampling-types 只在 Pager dev-dependencies 中，lib 测试图掩盖了生产引用；默认 CLI 检查与 all-targets 必须分别执行，core workflow 已有独立默认 build。
- MCP 工作仍在独立 `codex/mcp-policy-sdk-identity`，核对时 HEAD 为 `5b375b07c08ec113c06cb9fa426bd0353e114941`，不是候选祖先；既有 stash `29671011ad27541fa97a8cc649d63190532aed60` 未动。本轮提交不含 MCP 源码、SDK 适配或该活动 change 的修改。多轮交互仍暂缓，不能在版本说明宣称已完成。

## 发布材料与门禁

1. **版本准备尚缺。** locked metadata 与 Cargo.lock 的 57 个 workspace package 一致；33 个继承产品版本，24 个独立版本。workspace/CLI 仍为 2.4.0，`crates/codegen/shell/changelogs/2.4.1.md` 不存在。正式准备时更新产品继承链及对应 lock，保留独立版本包，并新增 2.4.1 说明、更新发布索引；不能改写 2.4.0 说明代替新版本。仓库旧 CHANGELOG 索引首项仍为 2.3.1；发布工作流的说明权威是对应 tag 内的按版本文件，不能靠索引首项推断实际发布版本。
2. **最终来源尚未定版。** 本轮不建 annotated tag、不调用 release workflow，也不改产品版本。完整 `scripts/validate-release.sh v2.4.1` 要求 clean tree、已有 annotated tag 指向 HEAD、版本相符及非空 notes；不伪造 tag 令它通过。与并行任务协调：本轮提交完成后由该任务按其用户授权统一推送 main。
3. **同 SHA 门禁必须真实完成。** release validate 的完整 SHA 传给 core、三平台协调/导出、Windows storage、OpenSpec 和 build；失败、跳过、取消都阻止 publish。active MCP 的未勾任务不会被 strict OpenSpec 自动当作产品就绪阻塞，隔离范围仍须人工核对。默认分支 dispatch 不额外证明所选 tag 就是 main tip，正式发布时仍要核对最终候选 tag 与预期源码。
4. **产物验收仍待执行。** 十个平台构建、可运行目标 smoke、macOS 签名、Windows grow.exe、tar 单入口、attestation 和 SHA256SUMS/远程下载重验由 release 工作流完成后才转 public。riscv64、两种 musl 与 OHOS 的 smoke:false 限制仍在；本地 macOS check 不验证 Windows/PowerShell、其他 native cfg 或这些平台的实际运行。

## 本轮验证范围

默认 CLI 红绿、workspace/all-targets、18 项 Python 维护测试、工作流/脚本静态契约、文档相对目标和版本核对见 [verification](verification.md)。此前行为回归与真实 provider 原始证据见 [图片投影验证](../2026-10-19-hide-internal-image-projection-markers/verification.md)。本轮没有重跑真实 provider、PTY、release-dist 链接、十平台或托管门禁；历史 native release-dist 成功仅是前一项 remove-release-build-warnings 的验证，不能算作本次新候选产物已验收。
