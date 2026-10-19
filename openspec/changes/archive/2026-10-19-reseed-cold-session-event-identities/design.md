## Invariant and cause

进程计数器必须大于将被写入的 session 物理历史中已经使用的事件后缀。`DisplayProjectionExpected` 只覆盖部分事件；`replay_session_updates` 的 reseed 又被 noReplay 绕过。新采样复用普通历史行的身份后，exact append 正确拒绝，不能通过放宽冲突校验修复。

## Implementation boundary

复用 load 已读 updates 快照和已有 meta.eventId 解析逻辑，在 actor 发布、恢复通知或 startup projection 之前恢复计数。计数扫描保留 rewind 丢弃行，因为它们仍占据物理 exact-append 身份。Timeline-only 展示期待继续补足 cache 行未落盘的边界。客户端 replay 仍只控制展示，不决定 writer 身份资格。无新 registry、后台任务或额外全历史副本。

冷 writer 的 `load_light_data` 在恢复/重建任何可持久化投影前读取并保留一份解析后的完整物理 updates 快照。它从 ACP/Grow `_meta.eventId` 的最后一个 `-` 后缀取最大合法 `u64`，并与已验证 Timeline 中 `DisplayProjectionExpected` 的最大后缀一起推进进程计数器；同一快照继续供 response projection reconciliation 使用。不跨 persistence/bootstrap 传递额外状态；存储 owner 在首次可能写入前一次完成这个恢复职责。观察型 resident load 不读取物理更新来恢复 writer identity。exact append 实现保持原样。

## Verification

定向测试覆盖高序号普通 ACP/Grow 行、无更高展示期待、noReplay、新事件 exact append，以及 rewind/cursor 不降低序号。真实 normal/minimal Skill 挂起、改磁盘、冷恢复、导出之后执行 headless continue，验证原快照保留且新一轮持久化成功。保持 malformed history 和同 ID 冲突的原有拒绝规则。
