# Diagnose: workflow manifest exceeds 524288 bytes in `session::workflow::` tests

## Why

`cargo test -p shell --lib session::workflow:: -- --test-threads=1` 出现 25 个失败，统一报错
`Store("Workflow manifest exceeds 524288 bytes")`（见 `/tmp/grow-backlog-workflow-integrated.log`）。
主 `main` 上 Shell 96 全 pass，F4 change 已 archive 且真实 storage 5 个测试通过。
本 change 只做只读静态诊断，不修任何代码、不改 512 KiB 上限、不混入 F4。

## Scope

- 只读分析：tracker capture 路径、agent definitions snapshot（builtin + user/project
  discovery）、Skills discovery 默认 roots、本机真实技能文件 byte 统计。
- 不修改业务代码、测试、Cargo、全局环境变量、真实 `~/.grow` 技能文件。
- 不运行 cargo；不做网络访问；不读取真实 session history/keys。
- 产出：已确认事实（manifest 上限失败 + 测试读取本机技能）+ 尚待 isolated
  control 实测的越界构成归因；**静态诊断完成 ≠ 归因完成**。

## Status

已完成静态核查与固定测试产物的隔离对照：隔离子进程HOME/GROW_HOME后147项通过，原25项失败依赖发现环境输入。本诊断不改变产品容量、不实施新的快照结构；精确字段占比及真实大目录产品容量场景不在已得证据范围。记录隔离验证方法及独立测试维护债务后收口。
