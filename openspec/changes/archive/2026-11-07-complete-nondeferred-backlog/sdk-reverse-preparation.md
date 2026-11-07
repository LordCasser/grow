# MCP-F2 注册所属路由准备

本记录核对当前事实，未修改SDK或leader生产逻辑，未运行端到端复现。父已亲读`session/acp_mcp.rs`真实producer、`session/actor/spawn.rs`注册、`leader/server.rs`身份注入/target路由及SharedMcpPool；只读调查`01a1b2c3-aa9f-7762-ba9e-1f52888eaf23`补充new/load/resident与测试入口。历史旁支未合入，不整支搬取。

## 已确认事实

- `GatewayAcpInvoker`只持gateway；SDK reverse params只有`serverId`和`message`，没有sessionId或owner。它通过ExtRequest发送`grow/mcp/sdk_call`，上线为下划线扩展方法。leader无目标且无session的带id包会被丢弃，不走last-active通知fallback。
- leader `extract_target_client_id`支持params或嵌套params的`_meta.grow/leaderClientId`，优先级高于session driver；目标断开不fallback。可复用现单播路由，不需要另一个owner registry。
- 现`inject_capabilities_into_session_new`在new/load上仅在字段缺失时补`grow/leaderClientId`；existing test `inject_capabilities_does_not_override_existing_leader_client_id`保留caller值。这个值当前不能直接视为受信注册owner。若复用它，leader实际ClientId必须在该信任入口覆盖caller值，不能在消费者另猜身份。
- new和cold load从当前request meta解析SDK声明并注册；resident load只更新标准MCP服务器，`AcpMcpRegistry`保原值。现update_mcp_servers端点不接纳SDK声明，本修复不应扩大它。
- **继承纠正**：child传入空SDK声明Vec只说明child不新注册SDK，不代表不能继承已连接SDK transport。`McpState::build_pending_acp_clients`的注释和构造路径明确SDK client进入owned_clients；`SharedMcpPool::from_state`捕获all_clients，没有按transport类型排除SDK；child沿既有mcp_inheritance/ceiling过滤后可import这个Arc及其原invoker。只读报告中“child不继承/parentpool仅标准”的推断不成立，不作为设计前提。

## 后续最小边界

先在独立change冻结：leader按实际连接覆盖owner元数据，Shell把本次注册session及owner绑定到反向producer，沿现target-unicast完成路由；不得按serverId、last-active或driver推断注册者。当前每次SDK registry由同一new/cold-load请求声明、同一invoker服务整批条目，owner可绑定该invoker，未见需要为未来混合owner预建per-entry注册表的当前消费者。

常驻load维持原SDK注册及owner，B成为driver不应偷走A注册的SDK；coldload须使用本次新连接重新声明和绑定，不能恢复旧进程ClientId。继承的SDK Arc/invoker保原注册owner，而非改成child或当前driver。直接单连接ACP无leaderowner时仍使用本身gateway；实际leader入口保证补为服务器分配身份。生产实现前还需将这些决定写入独立delta/design，不能以本准备记录宣称已实施。

## 可复用验收入口

`leader/server.rs`已有persistent server fixture、connect/register拿ClientId、load/complete_load、driver转移、target单播和目标断开不fallback测试；它们目前用合成payload。必须新增实际`GatewayAcpInvoker`输出经leader的验证：A注册、Bdriver、A/B同serverId分别initialize/list/call仅到原注册者；owner断线无B回退；forged owner由实际ingress覆盖；resident不偷绑、cold新连接重绑；未绑定SDK的一般reverse仍走原driver。

不把手写_meta测试当真实producer覆盖，也不把标准HTTP/stdio路径通过当SDK reverse已修。权限、MCP-F1初始化后eligibility、目录名长度、rmcp版本及elicitation分别交付。
