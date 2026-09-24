## 验证结果

- `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo test --locked --offline -p tools --lib search_replace -- --test-threads=1`：101/101 通过，涵盖新建成功通知、既有空文件更新、非 NotFound 读取拒绝、真实子进程在读取与提交之间创建目标时不覆盖且不通知，以及无独占创建能力时不尝试普通写入。
- `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo test --locked --offline -p tools --lib exclusive_creat -- --test-threads=1`：2/2 通过，本地完整内容独占发布、既有目标和符号链接不覆盖、冲突临时文件清理，以及 MockFs 的原子存在性检查。
- 已用本地 `tempfile 3.27.0` 源码核对 `persist_noclobber`：macOS/Linux 优先 no-replace rename，不能用时以 hard link 创建目标名，均不会覆盖已有目标；整体操作在崩溃/清理失败时仍可能残留暂存路径，因此不宣称整个暂存生命周期原子。
- `rustfmt --edition 2024` 覆盖修改的 Rust 文件；`git diff --check` 与 `openspec validate --all --strict --no-interactive` 通过。
- 归档更新 `openspec/specs/tool-authorization/spec.md` 后，`openspec validate --all --strict --no-interactive` 19/19、`openspec validate --archived --no-interactive` 419/419 通过；`git diff --check` 通过。

## 剩余边界

既有空文件更新和普通替换仍由读后普通写入提交；ACP `writeTextFile` 无条件创建能力，因此确认缺失的创建现在明确失败。父目录替换、非协作写者在提交后再修改文件，以及普通替换的跨进程版本提交均不属于这次保证，保留在 backlog。
