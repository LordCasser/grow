# Avoid attributing old crash IPs to the current process

## Why

PB-06：raw v1仅保存绝对IP、pid/version，无module identity或load base；startup用backtrace::resolve在新进程映射解析旧IP。ASLR或升级后地址可未映射，也可落到无关函数，符号名不能当可信归因。pid/version相同也不能证明同一address space。

## What Changes

采用原backlog允许的“明确限制”边界：缺少已验证image/load映射的旧绝对IP只保raw，不通过当前进程resolver附会函数/文件行；报告明确说明未符号化原因和离线证据所需条件。保捕获signal/si_code/fault/raw IP/version/timestamp和原durable report/history。此项不预建module数据库、不改signal格式，未来有已验证module identity+relative address需求时独立扩展。

## Impact

crash-handler symbolicate/report及必要注释、已锁backtrace依赖的删除（仅在确认零其它消费后），对应真实PIE/跨slide与错误归因反例。需等PB-05写面释放后实施，不混入capture identity事务。
