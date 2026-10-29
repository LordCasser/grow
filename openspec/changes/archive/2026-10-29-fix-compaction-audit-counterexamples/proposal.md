## Why
全面审计后六个真实函数/actor fixture 探针复现五个缺陷。取消未信号化后台 worker 与现有 Late result invalidation 契约冲突；手动请求排队触发自动恢复空转；原始 reasoning 错误占用摘要来源预算；非成功 provider 终态被当作有效摘要；正文中的 analysis 结束标签导致前文丢失。

## What Changes
在现有 owner、选区、摘要 adapter 与格式化入口修复上述五个反例，保留明确失败与完整历史。将探针转为回归测试，验证共享摘要器和自动/手动/后台路径。不新建调度器、计数系统或重试策略。

## Impact
Shell compaction、summary adapter、ChatState 选区预算及 common compaction formatter。独立 Plan handoff 债务与未证实的组合风险不在本次实施范围。用户原请求已授权全面审计和修复。
