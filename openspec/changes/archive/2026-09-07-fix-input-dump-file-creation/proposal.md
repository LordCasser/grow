## Why
Input flight recorder 导出使用秒级时间戳命名并直接 fs::write；同一秒的第二次导出会截断覆盖第一份，权限也依赖默认创建模式。诊断快照应独立保留，失败不留下半份文件。

## What Changes
使用现有 tempfile 依赖创建带时间戳与随机后缀的私有文件，写完后保留路径；任何提交前失败由 owner 清理，目录创建错误正常传播。

## Impact
仅 input log 文件输出，不修改记录字段、脱敏、按键入口或 ring buffer，不增加依赖。
