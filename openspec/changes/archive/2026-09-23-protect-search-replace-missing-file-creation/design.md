## Context

`AsyncFileSystem` 只有普通 `write_file`，本地使用 `fs::write`，ACP 代理使用无条件 `writeTextFile`。路径检查、进程内 mutex 或提交前重读都不能把缺失目标的创建变为跨进程原子操作。

## Decision

新增语义明确的 `create_file_if_absent` 文件系统操作。`LocalFs` 在目标父目录中暂存完整内容，同步后以 `tempfile::NamedTempFile::persist_noclobber` 提交；目标已存在时返回 `AlreadyExists`，且暂存文件由库清理。`MockFs` 在一个写锁内做相同的存在性判断与插入。ACP 协议没有条件创建操作，适配器返回 `Unsupported`，工具给出可理解的失败结果，不发送 `FileWritten`。

仅在最初读取返回 NotFound 时调用该操作。读取到既有空文件仍走原更新路径；本次不声称它或普通替换有跨进程 CAS。父路径别名或符号链接在检查与提交之间改变时，独占提交仍禁止覆盖最终目标，但不能保证创建到最初观察的父目录；这一更广的路径身份问题继续跟踪。

## Verification

在真实本地文件系统上覆盖已有目标（含符号链接目标）及正常创建；用注入文件系统模拟读到 NotFound 后竞争者先创建，核对冲突、不覆盖和不发通知；验证 ACP 的 `Unsupported` 映射。运行定向 Rust 测试和 OpenSpec 严格校验。
