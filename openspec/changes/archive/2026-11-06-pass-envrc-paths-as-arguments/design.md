# Design: pass-envrc-paths-as-arguments

## 决策

1. 固定相对名而非 `$1` 传参。`Command::current_dir(dir)` 已使 bash 起始 cwd 即 `dir`，脚本源 `. "./.envrc"`：名字含斜杠，POSIX `.` 不做 `$PATH` 搜索；也避免把 `dir.join(".envrc")` 作为参数时相对 `dir` 被重复拼接。调用方路径字节（`$(...)`、反引号、引号、空格、换行、非 UTF-8）只经 `current_dir`/`env` 以 OsStr 进入子进程，永不进入脚本源码。
2. `PWD` 以环境变量导出而非依赖 bash 推导。无 `cd` 后 bash 会把 `PWD` 重置为 `getcwd()` 物理路径，symlink 化目录（macOS `/var` → `/private/var`）会使 `$PWD` 相对展开偏离调用方字符串，既有 `test_variable_expansion`/`test_path_add` 在 macOS 将回归失败。导出 `PWD=dir` 保持原可观察行为（旧 `cd "{dir}"` 后 `PWD` 恰为调用方字符串）；bash 校验不一致时回退 `getcwd()`，相对 `dir` 同样回退，无 `dir/rel` 加倍。
3. `PWD` 已在解析忽略项（`_`/`SHLVL`/`PWD`/`OLDPWD`）中，导出值不进入结果集；环境变量值经 `env -0` 的 lossy 解析为既有行为，不在本项范围。

## 保持不变

`load_envrc` 两层结构、direnv 成功路径、`load_envrc_or_empty(_when_trusted)` 签名与 trust 门控位置（调用方）、`set -e`、`DIRENV_STUBS`、`env -0` 输出、基线过滤。

## 不做 / 延期

envrc 子进程 timeout/cancel/output cap：独立延期边界（WS-05 已注明“独立评估”），本 change 不涉及；shell runner 重写、stubs 扩展、配置化无需求依据。
