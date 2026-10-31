# UI end-to-end verification

## Result

2026-10-31：真实 PTY 回归通过，1 个精确测试顺序执行 5 个独立场景，耗时 14.67s，所有 Pager 子进程干净退出（exit 0）。生产实现无需追加修改。

| 场景 | 观察结果 | 证据 |
| --- | --- | --- |
| 完整欢迎页 | 目标版本、ctrl+u 重启提示、即时 toast 同时可见 | [画面](evidence/welcome.html)、[文本](evidence/welcome.txt) |
| 完整会话 | 会话输入框就绪且 Starting session 消失后交付结果，立即显示目标版本与 restart to apply | [画面](evidence/session.html)、[文本](evidence/session.txt) |
| minimal | 空闲输入框就绪后，真实 native scrollback 显示 NOTICE 与目标版本、重启说明 | [画面](evidence/minimal.html)、[文本](evidence/minimal.txt) |
| 完整会话无更新 | None 结果后观察两秒，没有目标版本或更新通知 | [画面](evidence/session-none.html)、[文本](evidence/session-none.txt) |
| minimal 无更新 | None 结果后观察两秒，没有目标版本或更新通知 | [画面](evidence/minimal-none.html)、[文本](evidence/minimal-none.txt) |

HTML/文本来自 PTY 终端模拟器实际收到的输出，并非手写 UI。欢迎页和 minimal HTML 已在浏览器打开并截图目视核对。

## Commands

- `cargo test --offline --no-default-features -p cli --test update_notice_pty --no-run`：通过，生成 `target/debug/deps/update_notice_pty-0736d8db8182720c`。
- 同一产物 `--list --exact update_notices_render_in_real_terminal`：唯一匹配 1 个测试。
- 在 `crates/codegen/cli` 中直接执行同一产物：`UPDATE_NOTICE_ARTIFACT_DIR=<change>/evidence <test-binary> --exact update_notices_render_in_real_terminal --ignored --nocapture --test-threads=1`：通过。
- 随后 `cargo test --locked --no-default-features -p cli --test update_notice_pty --no-run`：通过，锁文件一致。
- Rustfmt 和 `git diff --check`：通过。

`--no-default-features` 省去与提示无关的 CLI jemalloc/sandbox-enforce 默认特性；测试仍安装生产的 pager-minimal hooks，并运行生产 Pager、嵌入式 Agent、事件循环及真实终端渲染。

## Fixture corrections and review

首次夹具从欢迎页一次性输入整段文字只进入了会话；后改为 Enter 打开会话并等待输入框。临时 home 的项目目录询问通过隔离 Git 仓库消除。空输入框不显示 Enter:send，故就绪标记改为实际模型栏并等待启动结束。

初版直接运行 Pager 库，没有注册 CLI 才安装的 minimal renderer，minimal 无画面。测试迁至 CLI 组合层并调用同一 `pager_minimal::install()`，不引入 pager 到 minimal 的反向依赖。以上均为测试夹具问题，未修改生产逻辑。

独立只读对抗审查确认：每场景独立 sandbox、HOME/GROW_HOME、mock provider；no-leader/trust；精确子测试入口；UI 就绪后 marker 握手交付更新；断言终端可见画面并要求正常退出。没有线上版本查询、下载或安装。

## Limits

从现有 oneshot 通道注入检测结果，覆盖结果交付到提示显示；不声称端到端覆盖 GitHub、版本检测与安装。None 场景为两秒观察，不具有 UI 消费 ACK。编译有未修改代码的 fetch_update 弃用警告及 macOS 大型 __eh_frame 链接警告，未混入本次验证修复。

## Archive checks

- `openspec archive verify-update-notice-pty --yes`：完成，skip_specs 不修改主契约。
- 归档后全量严格校验：16 项通过；archive 校验：760 项通过。
- 归档后格式与 diff 空白检查通过。
- 清理了本次已弃用的 Pager 测试 executable 与 dep-info，保留可复现的 CLI 测试产物；未清理其他任务共享缓存。
