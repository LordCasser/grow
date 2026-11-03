## 审查方法

以当前已归档 session-timeline 的观察不确定性、五秒控制查询、持久收尾责任、来源授权和采样独立性为基线。工具输出按结构化 status、正文、任务 identity、完成计数与实际等待原因交叉核对，区分查询完成、任务终态和结果可读性。

调查限定为 TaskOutput → ChannelBackend → coordinator 及 Shell history/progress/interrupted-wait 路径。Atlas 的局部结构证据只作定位，动态分派和同名函数以源码核对；不宣称全库调用图完整。

先记录可到达条件，再用生产公共 API 或既有 harness 注入单一故障。虚拟时钟验证等待而不实际睡眠；不通过放宽断言或增加真实任务来制造通过。故障探针确认当前问题后移除，不把错误行为固化成普通回归预期。发现的契约选择、可用性上限和已确认缺陷分开报告。

## 交付边界

本轮不改变 timeout、retry、状态 schema 或所有者结构。确认问题写入 analysis/verification，独立修复入口登记 backlog；后续实现须先建立对应行为 change，将反例转成断言期望行为的回归。
