# Verification

## 发布失败证据

候选 `f264f01d08143afbfdceb630eec64bf3acabde8b` 的 macOS job 118559346657：compile no-run 在 61 秒成功，随后 enumerate 再次编译 pager 并在 60 秒超时。Windows job 118559346869 同样先编译 91 秒，再次枚举编译导致超时。尚未执行导出断言，不能把这个阶段错误描述成产品导出断言失败。

## 正常与失败路径验证

完整 `PYTHONDONTWRITEBYTECODE=1 PATH="$HOME/.cargo/bin:$PATH" python3.12 -m unittest discover -s scripts/tests` 以及默认 Python 3.15：各 Ran 34、OK、skipped=1，即 33 通过与 1 项既有跳过，耗时 21.479s / 19.986s。

真实嵌套 Cargo workspace fixture 的 build.rs 记录每次执行，并修改 watch trigger。runner 编译/枚举/执行后只记录一次；额外独立 Cargo 调用会记录第二次，证明反例不是永远 fresh 的虚假夹具。另一真实 libtest 核对 current_dir 等于 CARGO_MANIFEST_DIR。

缺少或多个 artifact 拒绝；同 binary/cwd 两阶段；超过 64 KiB 的 artifact 不依赖诊断 tail；rendered 和 raw diagnostics 均保留。保留原 exact/ignored/zero-test/结果证明、独立 deadline、SIGTERM/interrupt/timeout 真实进程树清理。非 compile 的 partial-line cap 保持原值，compile 单条 JSON 则完整解析。Windows 路径断言按 Path 规范化。

## 实施边界

没有延长枚举期限或跳过测试。没有改 build.rs；Pager/CLI 失效追踪路径的独立债务记入 backlog。测试进程继承调用环境并使用 package cwd；本轮实际 fixture 未需要额外动态库或 Cargo runtime env 框架，正式原生与 PTY 门禁继续验证实际仓库用例。

旧官方 run 39402132260 已确认 completed/cancelled；v2.4.2 Release 仍为 404。Core 全量库回归已成功进入 CLI 构建阶段；linux-x86_64-musl、linux-riscv64 与 Windows storage 已通过；本轮不把这些结果复用为新候选发布许可。新候选重新执行完整同 SHA 门禁与十平台构建。
