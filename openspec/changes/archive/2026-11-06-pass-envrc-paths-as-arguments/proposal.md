# Pass envrc paths as arguments

## Why

Backlog WS-05（P2）：`crates/codegen/workspace/src/envrc.rs` 的 bash fallback（`load_envrc_via_bash`）把 `dir`/`.envrc` 经 `display()` 双引号插值进 `/bin/bash -c` 脚本。目录名含 `$(...)`、反引号时路径字节在 shell 源码中被求值（审计实验：目录名 `$(touch injected-marker)repo` 触发 `touch` 且加载失败）；含双引号等组合时拼接路径与真实目录不符导致加载失败；非 UTF-8 Unix 路径被有损替换后必然失败。trusted wrapper 的授权判定在调用方，本项不改信任边界。用户已授权全部非延期项，独立 owner 边界为 workspace envrc 加载器。

## What Changes

- 删除脚本中 `cd` 与 dir/envrc 插值：`Command::current_dir(dir)` 承担进目录，脚本为固定模板（`set -e` + DIRENV_STUBS + `. "./.envrc"` + `env -0`），仅 DIRENV_STUBS 参与拼接，调用方路径字节一律不进脚本源码。
- 以环境变量导出 `PWD=dir`（OsStr），保持 `$PWD` 相对展开（`PATH_add`、`$PWD/...`）解析到调用方传入目录；bash 在该值与真实工作目录不一致时回退 `getcwd()`，不加倍路径。
- direnv 成功路径与 `load_envrc_or_empty_when_trusted` folder-trust 门控不改；`set -e`/stubs/`env -0`/基线过滤保持。
- 不处理 envrc 子进程 timeout/cancel/output cap（独立延期边界），不新增依赖、配置或框架。

## Impact

- 代码：`crates/codegen/workspace/src/envrc.rs`（实现与同文件测试）。
- 契约：`openspec/specs/configuration-rules/spec.md` 新增 envrc bash fallback 路径处理 requirement（本 change delta）。
- 调用方零改动。验证：真实调用 `load_envrc_via_bash` 的特殊字符目录回归（标记绝不出现、唯一变量正确加载）加既有 normal/noenvrc/stubs 回归；cargo 测试由父在 main 共享 target 统一运行，本 worktree 不建 target。
