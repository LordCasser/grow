## 1. Portable fixture setup

- [x] 1.1 将 SessionHandle helper 改为平台绝对临时目录，运行使用该 helper 的 MvpAgent 测试及协调测试。
- [x] 1.2 通过原目录能力写滞后 Summary，保留活 writer 与 observer 全部断言；运行该精确存储测试。

## 2. Validation and record

- [x] 2.1 记录 Windows 失败与本机验证、完成 fmt/diff/OpenSpec strict 与 archive 校验后归档。
- [x] 2.2 提交并在 main push 协调 CI 验证 Windows 原生测试及真实进程脚本；记录远端结果。
