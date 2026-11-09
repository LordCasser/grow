# Report plugin uninstall failures

## Why

AC-08 卸载独立边界：CLI 共享 application 和 ACP actor 都将 repo removal/save 错误降为 warn 后返回成功；已有坏 registry 还会按 empty 处理。结果可能是文件仍在但记录丢失，或文件已删除但 durable 记录仍旧。

## What Changes

卸载在副作用前 fallible load，删除失败保原记录；仅确认 repo path 已移除后删除登记，并在 save 成功后报告成功。保存失败明确已移除文件、登记未完整提交的部分完成，不伪造 rollback。ACP 复用共享 application，保其原不清理 plugin-data 政策；CLI keep_data 与现数据清理政策保持。本项不实现安装/更新/marketplace 事务。

## Impact

Shell plugin uninstall application、actor Uninstall 与 CLI error consumer及相关测试。新增错误分支仅表达当前操作已可发生的失败，不新建 installer 框架或全局事务。
