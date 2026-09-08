# Design
spawn_transport_liveness 在同步入口读取 revision 并取得 Weak，然后 spawned task 不捕获强 Arc。tick 分支 upgrade，失败时通过既有 token 保护的 clear_liveness_slot 清理后退出；成功只在本次状态检查和事件处理期间持有 Arc，Healthy continue 后释放。

强引用不由长期监测任务延长，实际工具调用仍可按自身生命周期持有连接。slot 由任务保留至下一 tick 清理，不承诺 drop 立刻同步 join 任务。回归通过 Weak::upgrade 检查任务未首次 poll 前释放最后外部 Arc 后连接立即可销毁，并验证下一 tick 清理槽位且不发事件。
