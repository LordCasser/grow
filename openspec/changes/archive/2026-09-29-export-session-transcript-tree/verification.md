# 实施与验证记录

日期：2026-09-29。

## 来源与内容

- `cargo test --locked -p shell --lib session::storage::transcript::tests`：5/5 通过。fixture 覆盖 root ledger 持有 child/grandchild spawn、直接安全父层级、指定 child 为根、缺失或损坏 child/owner seed、缺失 updates ledger 拒绝空成功、稀疏超限源在物化前拒绝、空节点只读及源文件前后内容不变。
- `cargo test --locked -p shell --lib shared_read_session_resolution_stays_out_of_the_writer_cache`：1/1 通过。已有 writer 时 shared-read observer 不进入 writer cache；新 reader 只使用这一入口及 validated Timeline/reconciliation，不调用修复或执行入口。
- `cargo test --locked -p pager --lib export_cmd::tests`：6/6 通过。真实 Summary/Timeline/updates fixture 导出父子不同正文、直接 child 链接；文件、空目录、非空目录、符号链接拒绝覆盖；单文件 TUI 导出失败清理和权限行为保持。fixture 同时比较 CLI 写树与 trajectory 临时树，解包生产 tar.gz 后比较顶层目录、路径及正文。
- `cargo test --locked -p pager --lib scrollback::export::tests`：4/4 通过。完整 Markdown 保留 thinking、工具结果、Hook、fence 与快照未完成工具标记；旧紧凑 renderer 保留。
- `cargo test --locked -p pager --lib acp::tracker::tests -- --test-threads=4`：161/161 通过，含 display text、隐藏用户回显、已接受/丢弃 attempt、thinking、工具结果及 Hook 去重等共享展示回归。
- 代码审查确认树枚举只沿验证后的 spawn/seed/result 身份链，按 security parent 组织；fork/resume 来源不是目录边；每节点使用同一只读 reader、ACP tracker 与 Grow 投影。Timeline/updates 读取前后核对同一文件长度，变化时拒绝成功并提示重试；source 与 Markdown 各有显式上限。跨活跃 ledger 不声称全局原子快照。

## CLI、HTTP 与交互回归

- `cargo test --locked -p shell --lib token_routes_serve_both_page_forms_and_the_api`：1/1 通过。token/Host、安全响应头、绑定 session、busy、下载流持有产物及取消清理经 HTTP/handler 测试；页面入口不读取当前筛选或分页。
- `cargo test --locked -p pager --lib app::root::dispatch::tests::transcript`：36/36 通过，包含 TUI `/export` cwd、child 视图归属、历史加载、异步文件队列与文件权限回归。
- `cargo build --locked -p cli --bin grow` 后用隔离 `GROW_HOME` 和真实二进制 smoke：默认在命令 cwd 创建 session ID 目录与空 transcript，第二次同目标拒绝；`--clipboard`/`-c` 被 Clap 拒绝；`grow export --help` 显示目录参数。入口不加载模型配置。
- `cargo check --locked -p cli`、`git diff --check` 通过。首次构建前检查磁盘，结束后清理本轮 Cargo target。

## 验证边界

没有运行 512 MiB 实量压力测试或浏览器像素级视觉测试；预算分支、HTTP 下载与页面链接分别通过代码检查、定向测试和 HTML 检查。历史可选附件没有打包二进制，Markdown 仅保留已记录引用与不可用说明。

## 归档

- 归档前 `openspec validate --all --strict --no-interactive`：16/16 通过；`openspec archive export-session-transcript-tree --yes` 已将 delta 合入 `client-surfaces` 主规范。
- 后续 replay change 的依赖链接已指向本归档目录；归档后全量规范校验 14/14 通过。`cargo clean` 清理本轮编译产物 58.9 GiB，磁盘可用量恢复至约 55 GiB。
- `openspec validate --archived --no-interactive` 在归档动作后首次检查只因本条“归档后校验”任务尚未勾选而报告 12/13；完成实际检查并勾选后复验结果见最终校验。
- 最终复验：`openspec validate --all --strict --no-interactive` 14/14、`openspec validate --archived --no-interactive` 553/553，均无失败；`git diff --check` 通过。
