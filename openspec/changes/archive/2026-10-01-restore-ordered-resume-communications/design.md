## 证据

当前 `replay_session_updates` 重建完整 updates 后，`PublishCoordinationState` 调用 `publish_parent_message_receipts` 将所有 Timeline 回执重新发到末尾。child 的 `replay_inherited_updates` 同样先 stream ACP 再 stream 通信。离线 `restore_observational_facts` 也将 missing receipt push 到末尾。稳定 `parent-message:<receipt_id>` 已防止同 receipt 重复，却不能修复位置。

用户来源最新输入之后存在 1266 条 admitted response 和工具历史，缓存并未缺失这些 response。12 条 reply 的 `reply_to.message_id` 都能关联原通信 ToolCall；集中 notice 是恢复规划错误。不能以隐藏所有通信或改变 Goal 行为掩盖。

## 实现原则

在已验证 Timeline 与 pinned updates 上构造统一的被动通信投影及历史插入位置。优先保留已有 receipt 的位置；缺失 receipt 通过 Timeline seq 和现有 canonical response/input/tool identity 的因果锚点插入，而不全量按墙钟排序、不按文本猜测身份。原逻辑序列不重新排列；缺少锚点的降级必须明确且有界。保留原来源时间供 replay，不用读取时刻冒充。

根 load、child stream 和 export/replay 调用相同只读规划，避免一个入口修好、另一个仍尾部补录。加载后的 snapshot 只用于当前协调状态，不能再把整份通信历史追加一次。增量 reconnect 要维持 exact cursor、完整替换回退和 receipt 去重；历史生成记录不能成为不存在于源缓存的 reconnect cursor。接收方 inquiry 已存在的 received/approval/completed 行保持原位，只补缺失阶段，身份使用 source peer + inquiry ID + phase；发送方审计沿原缓存规则，不扩展本次恢复范围。既有通信行通过匹配 Timeline 身份成为可靠锚点，不能信任任意缓存序号或跨 peer 合并。

正常显示组件仍按同一 Notice/communication body 展示保存的信息，不创建恢复说明行、权限应答、通知消费、Hook 或 provider 行为。测试使用隔离来源及纯 load/投影路径，真实活动 Goal 来源只读核对，不直接 resume 执行。

## 验证

复现两段正文中间的 Timeline-only reply，断言 full resume/child/export 的 receipt 位置、稳定身份、原时间和最终内容一致；重复/增量 reload 不追加副本；空缓存/缺 payload/rewind/晚到记录 fail-closed 或既有显式缺失语义。用真实用户来源核对 12 条 reply 不再成批尾部出现、最新正文与终态不丢失。同步文档与规范。
