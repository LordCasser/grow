## Why
Plan恢复在reconcile成功后再次读当前artifact；二读失败时无条件finish Plan，旧任务可终结替换后的新Plan。外部malformed响应保留pending候选，但现有显式恢复入口的同连接重试边界缺少直接回归证据。

## What Changes
同一恢复尝试从一个BehaviorSnapshot读取/校验原artifact，保留读取结果并核对snapshot，缺失/损坏只按exact snapshot收敛；不从可变当前Plan重新取正文。malformed只关闭当前interaction attempt，保留候选供显式RestorePlanApproval（session/load）重新挂起，不自动重试或新增产品API。

## Impact
Shell Plan恢复、已有typed交互与测试；不改变审批决定、Control、保留候选及handoff契约。
