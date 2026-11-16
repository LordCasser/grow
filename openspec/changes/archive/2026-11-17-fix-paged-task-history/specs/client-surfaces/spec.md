## ADDED Requirements

### Requirement: Paged history restores task state independently of transcript pages

分页load SHALL 从同一选定Timeline cut恢复后台命令的Tasks状态，任务行 SHALL 不依赖开始或终态所在正文页是否已加载。此恢复 SHALL 不执行命令、重放正文、推进正文cursor或native commit，亦不覆盖已保留的更新live任务事实。

#### Scenario: Task start is outside the latest page
- **WHEN** 挂起结算命令的开始记录在旧正文页，终态在所选cut内，客户端只加载最近页
- **THEN** Tasks历史恢复同task/tool-call身份的终态和真实记录耗时，正文仍按有界页展示。

#### Scenario: Task state replay is repeated or followed by an Around page
- **WHEN** 同cut任务状态重复到达，或时间线跳转替换已加载正文区间
- **THEN** Tasks不增加重复行、不退回Running、不丢保留输出或取消状态；正文页独立安装且native提交事实不变。

#### Scenario: A stale unfinished task is reconciled after paged load
- **WHEN** 同cut任务状态中保留未结算start，随后既有冷恢复reconciliation送达终态
- **THEN** 中心任务行正确终结，state-only恢复不先追加一份正文或完成通知。
