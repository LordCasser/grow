# Verification

## 范围

起点 main `d9ebf99c`，工作区干净。本轮只有 13 份现行 Markdown 说明和本 change 的文档记录变化；不修改 Rust/配置/依赖/主规范/CI、产品版本或 2.4.0 发布能力。具体逐项发现和源码证据见 audit.md。

## 已执行检查

- 独立静态核对当前 README/docs/Pager docs/2.4.0 notes 的 64 份 Markdown：**556 个相对或本地链接、317 个标题锚点，缺失文件 0、坏锚点 0**。修正了 3 个失效锚点；新增 suspension-and-recovery、local-todos-and-delegated-work 和 canonical spec 目标已单独复核。没有据此宣称远端 URL 当前可达。
- `cargo +1.93.1 metadata --locked --no-deps --format-version 1`：继承 workspace version 的 **33** 个 package 均为 **2.4.0**，另 **24** 个独立版本按各自 manifest 核对；与 Cargo.lock 无差异。版本结果 `/tmp/grow-v2-4-0-doc-audit-versions.json`。历史文档旧版本保留，当前 Goal architecture 明确与代码常量 v10 对齐。
- 执行 `scripts/validate-release.sh` 的 Python 静态契约段，对 release.yml / build-one.yml / 2.4.0.md 的平台 asset 列表、workflow 和 notes 约束校验成功，日志 `/tmp/grow-v2-4-0-doc-audit-release-static.log`。没有运行要求干净树及 annotated tag 的完整发布入口，没有创建 tag 或触发发布矩阵。
- `git diff --check`：通过。
- `openspec validate check-v2-4-0-release-docs --strict --no-interactive`：通过。

## 限制

纯文档校对没有重跑 Rust 单测、PTY、真实 provider/MCP 或跨平台构建；不把既有验证冒充本轮新执行。实际运行时回归及其限制保留在先前已归档记录中。未完成的 `add-mcp-multi-round-elicitation` 保持 active，SDK/真实服务门禁不变。

历史 /tmp 日志路径不是仓库内永久证据，本记录保留方法、数量、发现及源码入口供复核。没有修改或补造历史运行结果。

## 归档前规范校验

`openspec validate --all --strict --no-interactive`：**17 passed / 0 failed**；`openspec validate --archived --no-interactive`：**607 passed / 0 failed**。所有列明文档修正已与主规范和实现复核，本 change 不更新主规范。

## 归档后复核

`openspec validate --all --strict --no-interactive`：**16 passed / 0 failed**；`openspec validate --archived --no-interactive`：**608 passed / 0 failed**。归档不修改主规范，活动目录仍仅保留暂缓的 MCP 多轮 change。
