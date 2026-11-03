# v2.6.0 发布准备验证

## 环境与对象

- 用户确认版本 2.6.0；基线 HEAD `bbd2c4dfa` / v2.5.5，最终对象是包含既有未提交工作的候选工作树，不是 immutable release commit。
- 使用 `PATH=/Users/lordcasser/.cargo/bin:$PATH` 的 Cargo/Rust 1.93.1；Homebrew 默认 1.99.1 未用于正式验证。
- Cargo jobs=4、`RUST_MIN_STACK=16777216`，复用共享 target。核心回归设置独立临时 GROW_HOME；各已提供进程隔离的夹具仍自行创建子进程 home，不运行真实用户任务或付费 provider。
- 首次 core 编译时为补充独立 GROW_HOME 主动 SIGINT 自己的 Cargo，退出 130；没有将这次未执行套件计入通过。未触碰已有其他 session/test 进程。

## 已通过

| 检查 | 实际结果 |
| --- | --- |
| Rust format | 初始既有候选不符合 rustfmt，按仓库固定工具链统一格式；最终 `cargo fmt --all -- --check` 通过。 |
| whitespace | `git diff --check` 通过。 |
| Release scripts | `python3 -m unittest discover -s scripts/tests`：34 tests，OK，1 skipped；未执行的环境依赖项仍为 skipped。 |
| Native vector | `cc -O2 -Wall -Wextra -Werror -DSQLITE_CORE .../native_boundaries.c -lsqlite3 -lm` 编译及执行通过。没有在本机宣称 Linux sanitizer 门禁通过。 |
| Version/lock | Cargo.toml 2.6.0；Cargo 离线解析只同步 33 个继承 workspace version 的锁定条目，无第三方依赖升级；cargo metadata 与 lock 对照通过。 |
| Release notes | 2.6.0 notes 存在，标题/版本一致，无 repository-relative GitHub Release 链接；明确旧 child 缺出生 MCP 事实的恢复限制。 |
| Markdown links | 活跃 README/docs/crate README/用户指南共 63 份文件、694 条相对路径，0 个缺失目标（不包括 fragment 正确性保证）。 |
| Shell/YAML | `bash -n scripts/validate-release.sh scripts/build-ohos.sh` 与 Ruby YAML parse（release/build-one/core）通过。 |
| Prompt | 独立修复回归先红后绿；Agent 全组 489 passed，tool-types 127 passed。真实 Context 与 Builder 覆盖见 prompt 修复归档。 |
| Metadata schema | 原 tools 全组暴露 schema stale。先用既有生成入口验证快照缺项；随后发现闭合词汇仍沿用 v2.5.5 的版本 5，由独立 change 升至 6。最终精确检查与构建结果在下方补录，不把中间 v5 快照验证当作最终 v6 验证。 |
| Production check | `cargo check --locked -p cli --bin grow` 通过，1m10s，无 warning/error 诊断。 |
| Workspace check | `cargo check --locked --workspace --all-targets` 通过，2m29s，无 warning/error 诊断；它不等于测试执行。 |

## 完整库回归的真实结果

执行与 Core workflow 相同的库集合，另加入本轮改动的 fsnotify/hunk-tracker/workspace：

```sh
cargo test --locked --lib -p chat-state -p sampling-types -p sampler -p memory -p workflow \
  -p shell -p pager -p pager-minimal -p agent -p tools -p tool-types -p diagnostics \
  -p pager-render -p tty-utils -p test-support -p fsnotify -p hunk-tracker -p workspace \
  -- --test-threads=4
```

第一次碰到旧 resume 文案断言；仅将该断言对齐本轮明确修正的文案语义，并保留 transcript、重建 runtime、不可复制 permit 和同类型要求。重跑后 Agent 489/489 通过。

后续联合执行在 Pager 失败而停止；剩余未执行库以 `--no-fail-fast` 单独执行，不跳过其中失败。18 个包的最终原始套件摘要如下（过滤掉内部子进程摘要，按每个包最后一份摘要计）：

| Library | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| agent | 489 | 0 | 0 |
| chat-state | 610 | 0 | 3 |
| diagnostics | 55 | 0 | 0 |
| fsnotify | 119 | 0 | 15 |
| hunk-tracker | 200 | 0 | 1 |
| memory | 302 | 0 | 0 |
| pager | 7555 | 11 | 15 |
| pager-minimal | 102 | 0 | 0 |
| pager-render | 1027 | 0 | 1 |
| sampler | 259 | 0 | 0 |
| sampling-types | 320 | 0 | 0 |
| shell | 4555 | 4 | 9 |
| test-support | 71 | 0 | 0 |
| tool-types | 127 | 0 | 0 |
| tools | 2430 | 1 | 7 |
| tty-utils | 29 | 0 | 0 |
| workflow | 65 | 0 | 0 |
| workspace | 875 | 0 | 0 |

合计 19190 passed / 16 failed / 51 ignored。tools 的唯一 schema 失败已修复并精确复验；未把这次单项结果改写成重新跑完整 tools 套件。Pager/Shell 的其余 15 项联合门禁失败记录仍保留。

## 失败核对与复现

- Pager 11 项均属于 `acp::spawn::tests::mcp_hot_reload`：global/project catalog、cold load、辅助 policy、server identity、SSE tool schema、late/replaced project server、resident catalog 和 registration gap。常见最后状态是已有 client ready，但新增配置项 `session.enabled=false`；policy 用例保留旧 enable 状态。直接相关 Pager dispatch 和三份 fixture 相对 v2.5.5 无 diff，不能据此断言本候选无回归。
- 单项 MCP exact runner 在新构建上失败；切到 /tmp cwd 另一次变为 second session/new 超时；本轮修改前留存的测试产物和一次新构建单项运行可通过。留存产物没有重新证明 source fingerprint，只用于诊断，不作为发布基线验证。
- 临时 tracing 加 native `sample` 捕获 ACP LocalSet 的 `spawn.rs:489 → ConfigFileWatcher::apply_session_update → watch_registered_path → watch_cwd_dirs → AccessFilteredWatcher::watch → notify::fsevent::FsEventWatcher::{watch_inner,run} → Receiver::recv`。这证明动态 watch 可同步阻塞该线程，不能自动解释全部失败。临时 tracing 代码最终 diff 为零，未修改生产行为。
- Shell 三项 watcher 事件用例在全组失败。已编译原 binary 的串行 `config::watcher::late_config_directory_tests::` 为 4 passed / 1 failed，`config_watch_late_directory_and_recreation` 仍报 `project config event missing: Elapsed(())`；没有放宽期限。
- `suspended_child_cold_restore_retains_birth_authority_end_to_end` 在全组和独立 exact 运行中均于 `authority.cold_load` 的 10 秒 fixture 期限失败。独立组总时长 52.26 秒，不能把其中其他阶段通过当作整条出生权限恢复通过。
- 两项阻塞已登记 backlog，并在 review.md 中区分观察事实与未证实根因。当前不改变旧出生授权限制、不删除失败场景、不新增无界重试或超时墙。

## 本地构建与正式发布

本地 optimized release 构建、产物 smoke 和最后 OpenSpec 结果在下方补录。上表生产/all-targets 检查和全组库回归发生在元数据提升为 6 之前；最终针对变更再次检查，不把旧结果自动移用于新源码。

原生构建先后两次因 jemalloc 生成头文件缺失失败。只清理该依赖 release 缓存（53.8 MiB）后仍复现，排除单纯缓存解释。实际 `command -v diff` 指向 OpenHarmony SDK toolchains 的同名程序；对不同文件它返回 0，而 `/usr/bin/diff` 返回 1，致使 configure 报 nonexistent header unchanged。用任务局部 PATH `~/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin:$PATH` 后生成头文件正常并完成中间 v5 release 构建（3m45s）。未修改全局 PATH、SDK、依赖或 allocator 源码；最终 v6 产物仍须重新构建。

当前结论：**候选尚不能正式发布**。即使本地构建成功，仍需结清上述回归，再由 clean committed tree、annotated v2.6.0 tag 与 exact-SHA 官方 required gates / 十平台 build / asset provenance 完成正式发布。未运行 `scripts/validate-release.sh v2.6.0`：它明确要求 clean tree、annotated tag=HEAD；本轮没有满足此前置条件，也没有绕过。未提交、打 tag、推送、安装或发布。

## 最终 v6 候选复核

- 闭合 metadata 已在独立 change 中提升为 6；schema exact（不设更新变量）与新 kind exact 各 1 passed，完整 taxonomy 组 7 passed。
- 最终源码重新执行生产 CLI check（22.19s）和 workspace/all-targets check（1m12s），均通过且无 warning/error；没有把此前 v5 的检查结果冒充为最终验证。
- 最终 release profile 构建 24.86s 完成。产物 `target/release/grow` 为 Mach-O arm64，138800960 bytes；隔离 home 的 version/help 通过，报告 `grow 2.6.0 (bbd2c4dfa)`。codesign strict 校验为 valid on disk / satisfies Designated Requirement。
- SHA-256：`0b4198e16e53f3d0aa5d15ef89afa7486e19ab757afd69a36ae4a3120b512baa`。未打包为官方资产、未安装覆盖全局 grow。
- Python release contracts 最后设置 `RUSTUP_TOOLCHAIN=1.93.1`，确保临时目录中的 Cargo fixture 也使用指定工具链；34 tests，OK（1 skipped），20.024s。
- v6 后未重跑整个 18 包库套件；本次版本常量与 schema 的直接影响由上述 taxonomy/序列化检查覆盖。原 MCP、watcher 与出生权限恢复的联合门禁失败仍未解除，发布结论保持 blocked。

## 清理

最终验证后停止构建。多包 `cargo clean --profile dev --dry-run` 在共享 target 上长时间只扫描而未给出结果，已取消（只读预览，未删除产物）。改为按本轮开始时间和受影响包识别并删除 98 个本轮新建的 `target/debug/incremental` 目录，保留较早缓存、现有测试可执行文件及全部 release 输出；文件系统可用空间增加约 26.92 GiB。清理本轮 Python bytecode 和独立测试 home。清理后 release SHA-256 未变。

本 change 的完成指审查、对齐、候选构建与验证记录准备完成；不表示上述发布阻塞已修复，也不表示正式 release 已获准或已发布。

最终归档收尾：全量 strict Totals: 17 passed, 0 failed (17 items)；archived strict Totals: 828 passed, 0 failed (828 items)；format/diff 校验通过。资料归档不解除已记录的发布阻塞。
