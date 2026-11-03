真实删除仍有两次必要核对：lifecycle guard 下的无副作用 preflight 避免已知失败破坏 actor；teardown+drain 后的原 writer/quarantine 事务重新核对，覆盖 actor 在 preflight 后提交新引用的竞争。将既有 teardown 方法改为要求调用方持有同一 lifecycle guard，不新增第二把锁。

ACP error.data 以 grow/sessionDeleteRuntimeInvalidated boolean 标注本请求是否已使旧 runtime binding 不再可用；不等同于 writer 已停止或历史已删除。preflight false，进入 teardown 后的失败 true；未收到可信 metadata 为 unknown。Pager 用 Option<bool> 表示这三种现有事实，不创建新 pending 删除表。true/unknown 时通过既有 draft owner 捕获原 draft，并退休旧本地 view（含子 view、旧 interaction/路由 owner）；保留未删历史条目，随后 /resume 真正 load。这样不保留半解绑的旧子树或把同一份未发送队列复制到另一个 workspace owner。false 不改原 turn/owner。strict success 需要 success=true，坏响应不得当作删除成功。

成功先公共移除 matching logical id 页面并处理 draft boundary，再选择 after 的落点。Stay 对未删的活跃页面、Welcome/Dashboard 和现有 picker 保持；当前 Agent 被删则回有效 peer 或 Welcome/已有 Dashboard。其他身份页面与草稿保持。每次请求携带原 AgentId/binding epoch 快照；late result 只能影响仍匹配该 snapshot 和目标 id 的页，不能因同 id 重新打开或重新绑定而退休新页面或清除它的 registry。

Tier 3：公共 ACP 请求证明 preflight 拒绝保留 actor、writer 在 shutdown 时新增引用导致 late refusal并带失效事实；同一 lifecycle guard 贯穿存储结束。前端验证 true/false/unknown、严格 ACK、显式 resume load、after=Stay retirement与无关 draft/picker保留。所有公共存储测试用隔离 GROW_HOME/临时根。

成功 wire 沿用现有 Delete raw `{success:true}`；Close 仍是 ExtMethodResult 包装。各自按实际协议解析，不增加双格式 fallback。
