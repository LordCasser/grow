## Mechanism and minimal fix

DUMP_BASH_STATE_SCRIPT 的 local content/env_vars/all_functions 在 set-a 下带 export 属性，外部命令需要继承它们，大小受操作系统 exec 限制。本机受控 1.2MB 函数在普通模式 dump 成功，set-a 模式报 Argument list too long。临时只关闭 serializer 的 allexport，捕获原 flag，生成其明确恢复命令；采集 shopt 输出时不把暂时关闭后的 flag 当作用户选项。序列化 local 显式使用 +x，避免继承同名用户全局变量的 export 属性。

原快照先恢复 POSIX flags 再还原函数/别名，会让 grow_snap_* 在恢复期间被导出。保持其他 POSIX flags 与 shopt 的原先顺序，只在全部载荷之后单独恢复 allexport；否则 posix 会重置合法的 inherit_errexit 等 shopt 组合。继续使用现有字段、base64 编码与 fd 3/4，标记和 parse_dump 不变；调用结束后原 shell 进程即退出，持久语义由快照恢复。Zsh serializer 有独立 emulate 选项边界，不作顺带重构。

验证普通与 set-a 下大型函数 dump/replay、用户导出值和函数保留；旧 set-a/临时命令变量与失败命令回归继续执行。Ubuntu 门禁的实际 stderr 单独记录，不将机制复现说成已验证 hosted 通过。
