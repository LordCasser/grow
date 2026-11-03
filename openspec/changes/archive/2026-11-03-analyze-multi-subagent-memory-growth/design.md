# 分析边界

从进程整体 gauge、持久会话身份和实际所有者三层交叉核对。memtrace 的 footprint、RSS、allocated、resident 分别保留口径；磁盘日志大小不当作 heap 大小，purge 前后变化不当作对象释放量。

源码沿 Task coordinator → Shell child → SessionThread/persistence，以及 Spawned/Finished → Pager child view → scrollback/render cache 两条路径检查正常结束和失败收尾。当前工作树与 HEAD 均核对相关保留结构；运行中的旧二进制只有版本证据，不推断其精确源码身份。

当前规范要求保留 canonical child result、持久收尾责任及恢复后的完整历史，不能为降低内存直接删除事实、伪造 retirement 或跳过 replay。后续修复应分别落在前端重状态的卸载/重建和 coordinator 的物理 owner 容量核算，不引入新 Agent loop、全局清理器或超时后丢弃责任。

没有旧进程的 allocation stack，也没有记录到 50G，故本轮不宣称完成根因复现。验证方法及可证伪条件见分析报告。
