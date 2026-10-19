# Verification

## 最终范围

版本保持 **2.4.0**。用户最终确定 Ctrl+C 保留输入框及当前回合取消行为、绝不关联退出；默认 Ctrl+D 一秒内连续两次确认退出，确认后挂起整棵会话树。行为 change 已归档为 [separate-cancel-from-session-exit](../archive/2026-10-19-separate-cancel-from-session-exit/verification.md)，主契约为 `client-surfaces` 的 Confirmed quit requirement。

此前选择性提交 `2e69e5db` 排除的下列四项现已全部纳入本次最终工作区，保留它们各自的实施与验证记录，不改写历史范围决定：
- `2026-10-18-complete-image-fallback-lifecycle-regressions`
- `2026-10-18-harden-image-fallback-routing`
- `2026-10-18-verify-current-model-tool-image-fallback`
- `2026-10-18-fix-blank-ask-session-inquiry-id`

MCP 多轮交互仍因官方 SDK/真实服务门禁暂缓，唯一相关 open change 保留。Remote Control / Agent Host、依赖真实路由实测的 Provider cache / Recap 方向仍暂缓。没有新增这些方向的实现。

## Git 与版本核对

- `git fetch origin` 成功；fetch 后 `main..origin/main` 没有 incoming commit。
- 收尾前 local main 为 `8cc20454`，origin/main 为 `0a0dd07c`，local main 是 `codex/v2-4-0` 的祖先；不存在需要解决的远端分叉。
- Cargo workspace 共 57 个 package，其中继承 workspace version 的 33 个均为 2.4.0；其余 24 个按各自 Cargo.toml 独立版本核对。metadata / Cargo.lock 对全部 57 个本地 package 无差异。不把独立库强行改成产品版本。
- 现有 35 个 tag 收尾前保留快照 `/tmp/grow-v2-4-0-finalize138-git-before.json`；本轮不创建 tag、不 push、不触发 GitHub 发布或安装。

## 现行文档一致性

- 更新 Getting Started、快捷键、配置、会话/Replay、Dashboard、first-prompt 教程及默认 registry 的提示，统一 Ctrl+C 局部/取消与 Ctrl+D 确认挂起退出。
- 退出默认键在所有终端为 Ctrl+D；非 VS Code 的 Ctrl+Q 仍可作为 alias。默认半页向下为大写 D，避免 Ctrl+D 被 scrollback 消费。
- 核对并补齐 2.4.0 发布说明中的图片请求/工具结果回退、冷恢复与中断清理、空 inquiry ID 规范化；明确 MCP 多轮暂缓。
- 修正现行协调架构说明中 session/peer 身份边界；修复开发文档、Hook 示例及 Pager motion 的失效相对路径。历史审计资料保留其当时事实，不视为当前行为权威。
- 归档按键 change 后，现行指南的引用已指向 canonical `openspec/specs/client-surfaces/spec.md`；新 requirement 的锚点存在。
- 最终链接检查覆盖当前 docs、Pager docs、主规范、活动 changes 和 2.4.0 notes 共 90 份 Markdown，467 个相对链接目标存在、0 缺失。历史审计中的 123 个绝对源码/临时日志引用不在相对链接核对范围；没有据此宣称历史临时文件全部可用。记录 `/tmp/grow-v2-4-0-finalize139-doc-links.json`。

## 合并工作区验证

- 固定 Rust/Rustdoc 1.93.1、两编译任务、16 MiB 栈，禁用 incremental/dev/test debug。为排除先前隔离快照与 root 共享 target 的 stale artifact，已定向清理相关旧产物并从完整工作区重编；细节和首次失败记录保留在按键 change verification。
- tool-types **123 passed**、sampling-types **308 passed**、Shell **4254 passed / 0 failed / 5 ignored**。
- Pager 最终全量 **7504 passed / 0 failed / 12 ignored**。
- 真实 PTY 最终 **6 passed**：运行根节点挂起/命令单次中断与冷恢复、root/child/grandchild 树恢复、普通/minimal Skill 原始正文及身份恢复、minimal 双 Ctrl+D 确认退出、Ctrl+C 流式取消后继续发送。首次 minimal Skill 快照断言的偶发失败未定位根因，单项和全组复测通过；保留失败与复测日志，未放宽核心断言。
- CLI 构建成功，`grow --version` 显示 2.4.0（编译时 HEAD 2e69e5db）；最后追加的测试环境、诊断和文档不修改产品代码。
- `cargo +1.93.1 fmt --all --check`、`git diff --check` 通过。
- 按键 change 归档后：`openspec validate --all --strict --no-interactive` **17 passed / 0 failed**；`openspec validate --archived --no-interactive` **606 passed / 0 failed**。
- 直接执行 `scripts/validate-release.sh` 的 Python 静态契约检查，校验 release/build workflow、平台 asset 列表及 2.4.0 notes，成功。脚本完整入口要求已提交干净树及指向 HEAD 的 annotated tag；本轮不创建 tag，因此未执行完整 tag 预检，也未以静态子检查代替真实发布矩阵。

本轮没有新增真实 provider 互操作、Windows/Linux PTY 或跨平台发布构建。此前各 change 的限定验证证据继续有效，不扩大成本次未执行的测试承诺。

## 最终提交与分支操作

待第一笔包含全部实现、规范、归档、文档及本记录的提交完成后，fast-forward 合入本地 main，仅删除已合并的 `codex/v2-4-0`。完成后在本节追加真实 Git 结果，再勾选末项并归档；不提前宣称该动作已完成。
