## Design
TranscriptBuild 增加 restart_after_reload 标记。唯一生产 begin_session_reload 循环完成后，对本轮 reload_agent_ids 中的构建 owner 标记并清空旧 IDs/正文。take_minimal_transcript 在 owner reload 活跃时返回 None 并保留请求；reload 结束时若标记重建则从最终正文重取 IDs、next=0、out 为空后交给 pump。新请求在 reload 期间创建为待重建。由入口置位覆盖两帧间完成 reload，无需额外 generation 或全量正文 clone。

## Validation
确定性测试在部分 build 后开始 reload，确认取出被暂停，分别执行完整 replay、cursor 成功和失败回滚后检查所有最终 ID、空前缀及 next=0；另覆盖 reload 期间新请求和无关 agent 切换。
