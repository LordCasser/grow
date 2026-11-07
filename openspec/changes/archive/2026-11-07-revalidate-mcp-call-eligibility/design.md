# Design

## 已核实的当前链路

`McpErasedTool::run`（servers.rs:1784）在本 session 的 `McpState` 下检查 disabled 并捕获 Arc；`try_call_tool`（2017）固定 admitted timeout/deadline、等待 `ensure_initialized`，再直接进入单次发送。`is_current_client`（2098）只保护失败后的 reset。Shell `extensions/mcp.rs::call_mcp_tool`（623）同样在初始化之前核 disabled、显式 endpoint 和当前 client，再调用低层 `McpClient::call_tool`（4122）；该低层函数没有 session state。因此两个实际入口都缺初始化之后的资格复核。当前仓内该低层函数的非测试调用方是 Shell direct；rmcp Peer 的同名方法不是该接口的调用方。

McpState 拥有 owned/shared clients 和本地 disabled；`get_client` 以 owned 优先。`SharedMcpEligibility` 拥有实时 ancestor chain 的 client binding 和 tool ceiling，`contains_tool` 会穿过 upstream；child map 的 reconcile 可以晚于祖先撤销，不能依赖 map 已及时删除。owned 同名覆盖是独立来源，不能被无关 inherited 撤销连带拒绝。现 main spec 的 late-error 条款已符合 no-replay，不需要照旧审计恢复过时的重试文字。

## 决策与接口

MCP call owner 在最后异步准备结束后作资格决定，不在 Shell 外围提前检查后让低层再异步初始化。复用现 `McpState` 和继承权威，不增加 permits、公共错误类型、generation registry 或重试层。

在 servers.rs 的 `McpState` 增加私有共用方法，概念签名为：

```rust
fn validate_tool_client(
    &self,
    server: &str,
    tool: &str,
    expected: &McpClient,
) -> Result<(), McpError>
```

它按当前状态拒绝本地 disabled、当前 client 不存在或与 captured `expected` 不是同一实例；safe pointer equality 或现 client incarnation identity 均可，不能只比 name/config。若选中的是 shared client（不存在同名 owned 覆盖），按现 inherited authority 核 exact client binding 和 qualified tool ceiling；不等待 intermediate reconcile。复用现错误 `McpError::ClientError`，明确 pre-dispatch rejection；模型路径映射到原 ToolError 类别。不把“未发送”伪装成 outcome unknown，也不因此触发 reset/reconnect。owned 路径不新增一个要求 tools/list 已发布的目录 gate，保原 direct 首次调用用途。

`McpErasedTool::try_call_tool` 在初始化返回后取得现 state、调用该方法，释放锁后沿原 absolute deadline 进入一次派发。锁只保护当前资格采样，不跨 handshake、response 或网络等待。此边界是派发准入决定；之后已发请求的撤销仍按现取消/不确定结果处理，不承诺回滚或对任意网络排队点实现新事务。

低层 direct 方法同步更新为：

```rust
pub async fn call_tool(
    &self,
    mcp_state: &Arc<Mutex<McpState>>,
    tool_name: &str,
    arguments: serde_json::Value,
    timeout: std::time::Duration,
) -> Result<rmcp::model::CallToolResult, McpError>
```

Shell 传自己已有的 exact `mcp_state`，其显式 URL/pre-admission 检查保持。低层方法在本身 `ensure_initialized` 后复用同一 validator，再沿原 admitted deadline 进入发送。测试调用方也提供真实所属 state；不保留绕过 state 的 compatibility overload。无需让 client 保存另一个 state/closure，也不把 Shell callback 注入 MCP 形成反向依赖。

## 必须保持的边界

仅禁止准备期间已经撤销的调用：不按 name 切换新 client，不自动发第二次 tools/call，不因 pre-dispatch rejection reset旧/新transport。初始化、资格锁等待和派发共用原 admitted deadline，不在准备完成后重给完整timeout；post-init 获取异步状态锁也在该 absolute deadline 内等待，期限耗尽按既有 pre-dispatch timeout 明确失败而不发请求。已有 originating-peer cancellation、三秒通知派发上限、未知结果和晚错误不重置 replacement 的规则保持；server URL 检查不降级。MCP-F2 的SDK注册owner、目录名长度、工具权限和自动恢复政策分别处理。

## Coder Task Spec / Review Plan

一个 MCP call-owner coder 实施同一初始化后发送资格行为及其 Shell direct 参数适配；主写 servers.rs 与近邻tests，Shell extensions/mcp.rs仅传已有state和相关实际入口回归。不得整支合入历史 `codex/mcp-policy-sdk-identity`，也不改 rmcp版本、SDK注册、caller权限或新retry政策。

先用原实现建立真实可控 initialize/recovery 握手 barrier，captured client A 等待时分别 remove、same-name replace B、disable tool、revoke/replace ancestor authority，再释放。模型与 direct 均经实际调用入口验证A/B tools/call均0、明确拒绝；至少覆盖两种初始化来源（首次与恢复）而不是把所有fixture直接塞Ready。未变化控制发送一次，原deadline仍覆盖准备时间；无关server改动不能误拒绝。继承测试保持child旧map不reconcile，以证明live authority检查；owned同名覆盖作为对照。

旧红必须确实观察旧client发送，fixture编译/路径错误不算行为证据。Shell direct的现接口与server_url拒绝控制需经过真实调用方，不只低层helper。复用已有memory/duplex或loopbackserver test能力，用握手/计数证明；不靠sleep猜调用没发生，不新建大框架。对已发请求的cancel/timeout/no-replay、late-error/replacement跑已有定向回归。父亲核最终diff、实际入口、计数与deadline证据后同步开发说明/backlog并strict/archive；本项目不是Windows现场或远端生产服务验证。
