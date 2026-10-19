## Why

候选 8a5685a4 的 Ubuntu Core 中原 set-a 用例通过，新增大型 fd 快照用例在首次 state 更新断言失败（2424 passed, 1 failed, 7 ignored）。命令状态和函数输出已通过，尚无 dump 长度/尾部诊断，不能直接归因超时。代码另确认 reader 仅见 END marker 字符串就返回，而 parser 必须见末尾换行；流分块可确定性产生提前结束。累计全文扫描还具有二次复杂度。

## What Changes

reader 以完整末尾 marker 行为完成条件，复用已有五秒期限和 fd/parse 契约；用固定长度 suffix 检查避免累计重复扫描。增加真实 pipe 的 marker/newline 分离且 writer 保持打开回归，保留大型真实 fd 用例并增强无效 dump 诊断。核对 Ubuntu 最终执行，不能把已知边界风险直接宣称为上次失败的已证根因。不改 MCP、权限、编码或超时长度。
