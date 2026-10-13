# Verification

日期：2026-10-11。本次交付仅为 OpenSpec 立项和评审/roadmap 文档，不实现产品代码。

## Completed documentation checks

- 工作区预校验及写入仓库后的 `openspec validate --all --strict --no-interactive`：17 passed，0 failed（14个既有 spec + 3个新 change）。
- 仓库 `git diff --check`：通过。
- 新文档和两个现有文件新增部分的本地链接：17处检查，0处缺失。
- 初始109条工作树状态未移除；新增状态仅为3个 change 目录与上游评审入口。backlog 原已修改，本次仅追加 Remote/Agent Host 暂缓条目。
- 原有变更及目标文档共建立132份文件 hash；除明确授权更新的 backlog/评审入口外，原文件 hash 无变化。backlog 去除本次唯一新增条目后与写入前逐字相同。
- HEAD 仍为 `7b7d56ab98a686223bc659567c79c1363729cd66`；未修改主规范、产品代码、Git远端引用，未提交/推送。
- 新增24个 Markdown/YAML 文档，更新2个已有文档；所有文件均属于本次3个 change、backlog 或原评审入口。

## Limits and status

两个行为提案的实现任务和 acceptance 用例均未执行；没有运行 Rust/PTY 或真实 MCP 服务测试。仅文档核对/立项/校验任务可勾选，不能由本记录推导新产品能力已实现。

上游标记 `2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8` 仅为 MCP 与保留计划两主题的已评审目标，不是全提交审计、merge/cherry-pick、实现完成或 remote latest HEAD 证明。行为 change 保持 active，不归档未实现契约。

## Architecture review — 2026-10-11

后续复审已补充官方 rmcp/兼容依赖责任、SDK 接点验证、宿主调用/队列/准入边界和 M-15～M-25；详见 [实现责任](../archive/2026-10-11-audit-new-proposal-boundaries/implementation-boundaries.md) 与 [审计验证](../archive/2026-10-11-audit-new-proposal-boundaries/verification.md)。上方是初次立项的历史验证记录，本节没有宣称这些新增验收已执行；本 change 继续 active，未升级实际依赖或实现产品代码。

## SDK implementation stage — 2026-10-11

本节记录后续实际依赖升级，前两节是升级前历史。本轮官方 rmcp 从 2.2.0 升至精确固定的 3.5.1，适配 optional HTTP session id 和 ClientConfig，增加实际 SDK 自动 helper fixture。结论和源码映射见 [SDK 核查](sdk-integration.md)。

### Rust 与 SDK

初次检查使用 rustup Cargo，但编译器指纹揭示它仍调用 PATH 中的 Homebrew rustc 1.98.1；这些结果不计为 Rust 1.93.1 兼容证据。随后将 `.cargo/bin` 放在 PATH 首位，并显式指定 `RUSTC`/`RUSTDOC` 为 `~/.rustup/toolchains/1.93.1-aarch64-apple-darwin/bin/` 下的程序，重跑编译和完整 mcp 测试；target 指纹确认 `rustc 1.93.1 (01f6ddf75 2026-02-11)`。构建设置 `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 RUST_MIN_STACK=16777216`，使用本任务独立临时 target，不改已有 6.1 GiB 仓库 target。

- 初次 `cargo check --locked -p mcp`：通过；后续固定 1.93.1 的 cli check 同样编译 mcp。
- 初次 `cargo test --locked -p mcp --lib --tests`：原单测 174/174、原 SSE fixture 3/3、首次 SDK 探针 8/8。
- 增加现代 HTTP fixture 后，首次 `cargo test --locked -p mcp --test rmcp_sdk_contract`：9/9，通过；最终以显式固定 1.93.1 的完整测试复验为准。
- 显式固定 rustc/rustdoc 1.93.1 后，`cargo test --locked -p mcp --lib --tests`：174 个单测、3 个 SSE fixture、9 个 SDK 探针全部通过，0 failed。
- 显式固定 rustc/rustdoc 1.93.1 后，`cargo check --locked -p cli`：通过，覆盖 Shell/Pager/doctor 等 rmcp 类型消费者。Pager 的两项未使用方法产生一个 dead_code warning，位于原有工作树改动，未顺带处理。
- `cargo metadata --locked --no-deps` 和 rmcp/reqwest/http/sse-stream 的反向 feature tree：单一 rmcp 3.5.1，reqwest 0.13.4/http 1.5.0/sse-stream 0.2.5；未启用 SDK auth/macros/server。`cargo tree -i aws-lc-rs` 无匹配包，lockfile 也无该包。
- 一次全工作区 `cargo metadata --locked --offline --filter-platform aarch64-apple-darwin` 因未缓存的无关 alacritty_terminal 失败；不计为通过，不为此拓展到全工作区依赖下载/升级。

SDK 测试中的脚本 peer 是故障注入服务端，续接全部由官方 helper 执行。四项 `sdk_limitation_*` 成功复现调用级 capability 覆盖、整轮预检缺失、owner token 不传播、drop 不取消 wire 的限制；它们表示发现阻塞，不表示 Grow 对应契约已通过。

### 范围和门槛

M-24 本机依赖/trait/feature 与既有 SSE/取消回归通过，OHOS 交叉编译未执行。M-25 探针完成、接入门槛未通过，期限/手动检查/gate 的 SDK 接点也未取得。Shell/Pager 交互、真实 Form/URL 服务、HTTP-auth 和 ACP 新能力未实施或验收，生产入口未启用现代 discovery/MRTR。

只勾选版本/API 核查和依赖适配的已完成子任务。保留 tasks 2.0a/2.2b 与后续实现未完成状态；change 不归档，不提前修改主规范。上游问题只在本地记录，没有发布 issue/PR、提交或推送。

### 文档与改动隔离

- Rust 1.93.1 rustfmt 对本轮 Rust 文件的 `--check`：通过；`git diff --check`：通过。
- `openspec validate --all --strict --no-interactive`：17 passed，0 failed。
- 本轮开始时对 6309 个现有文件建立 hash；最终仅 10 个授权文件变化、新增 SDK fixture 和核查记录 2 个文件，无原文件删除。原有 Shell/Pager/Timeline 等在制改动未被本轮修改。
- 本次独立临时 target 编译产物已清理，已有仓库 target 保留。
