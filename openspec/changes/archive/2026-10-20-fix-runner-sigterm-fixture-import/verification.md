# Verification

官方 v2.4.2 首轮候选 `85eb50ab9b23fcdc705f97d5cc66b177dfe24d6f` 的 Core 与 Linux/macOS 原生门禁均在真实 SIGTERM 测试启动前失败；日志中 PID marker 不存在。Python 3.12 单独动态导入确认 dataclass postponed annotation 查找未注册模块时 AttributeError。

RED：Python 3.12 focused SIGTERM 测试 1 failed。GREEN：修复后同用例 1 passed（1.789s）。完整 `scripts/tests` 在 Python 3.12（17.447s）及默认 3.15（16.529s）均为 Ran 29、OK、skipped=1，即各 28 通过、1 项既有跳过；未将 skip 计为通过。使用 `PYTHONDONTWRITEBYTECODE=1 PATH="$HOME/.cargo/bin:$PATH" python3.12 -m unittest discover -s scripts/tests`，默认版本替换为 python3。

修复只在 exec_module 前注册动态模块，启动失败时打印 stdout/stderr/退出码；未修改真实 descendant、退出码 143 或存活 marker 断言，没有增加 skip 或延长期限。OpenSpec strict 17 passed；git diff --check 通过。

发布 run [39401343936](https://github.com/LordCasser/grow/actions/runs/39401343936) 已确认 completed/cancelled，v2.4.2 Release API 返回 404。未发布候选 annotated tag object 为 `fe5d277e4e58dbf76c5a7dfde5b328e52ac36a16`。后续提交经清洁 preflight 后以该精确 expected-object lease 替换候选 tag；新流程重新执行全部同 SHA 门禁与十平台构建，不复用旧源门禁或产物。
