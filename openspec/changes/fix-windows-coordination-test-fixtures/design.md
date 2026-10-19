## Context

Windows `Local agent coordination` run 39197297359 在 Shell coordination filter 中为 56 passed / 2 failed；新竞态回归三项通过。两个失败均在断言业务行为前的夹具构造阶段。

## Decisions

复用标准库临时目录，替换 helper 中四处不满足 Windows 绝对路径语义的 `/tmp`。对模拟旧 Summary 的测试使用 adapter 已持有的 bound session directory 和 `write_atomic`，与生产 Summary 写入的能力边界一致；不为模拟夹具改变 Windows 的目录 sharing 权限，也不释放原 writer 来绕过“观测活写者”场景。

保留 canonical title、observer 不持 writer lease、不改 Summary/Sideband、原 writer 存活时拒绝第二 writer、替换 writer 只追加一次终态的所有断言。不跳过 Windows，不扩大为其他测试或存储重构。
