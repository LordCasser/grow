# 2.6.2 upstream candidates — evidence review, 2026-11-06

用户要求对bot四项静态差异“分析并视情况吸收，一起作为2.6.2的一部分”。基线声明为Grow748bade / upstream2bdd1d6，不是当前工作树验收。Grow748bade本地是2.6.1发布说明提交；当前大量未提交修复不能从该摘要推断缺失。远程git ls-remote未成功返回，不称已同步upstream ref。GitHub固定blob `2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8` 已实际获取到Messages首替换/后追加实现，固定源码证据与main freshness分开记录。

| 候选 | 当前结论 | 实施/证据边界 |
| --- | --- | --- |
| rg hostname-bin | 已修、归档，纳入2.6.2工作树 | [真实helper执行、glob分隔符反例、19+2最终回归](../2026-11-06-guard-ripgrep-external-execution/verification.md)。safe/Auto/prefix和受限RX投影共用owner，exact invocation grant不改。 |
| MCP取消写等待3秒 | 已修、归档，纳入2.6.2工作树 | [32B真实受阻transport、旧红新绿16项](../2026-11-06-bound-mcp-cancellation-dispatch/verification.md)。只约束Grow任务，不承诺撤回SDK已入队发送或取消远端副作用。 |
| Messages signature fragments | 当前不盲目吸收，先保留官方SDK一致语义 | Grow保留start初值、后每个SignatureDelta赋值；官方TS/Python SDK同样覆盖。正式文档描述stop前特殊signature_delta且omitted模式为single delta。未取得同一thinking block多个语义signature事件确为片段的实际路由证据，不能把TCP/SSE字节分片当多个语义事件。见下方来源。 |
| MCP目录名256 / provider函数名64 | 已按[独立契约](../2026-11-09-separate-mcp-catalog-name-bound/design.md)实现并通过[真实入口验收](../2026-11-09-separate-mcp-catalog-name-bound/verification.md) | 普通Shell采样只取builtins，search_tool把qualifiedname当JSON值、use_tool内部dispatch；公开Agent::tool_definitions仍须保provider共同格式。先实现私有Agent投影64，再放宽MCP目录256；完整bridge保留，不截断/alias、不增公共helper或Agent→MCP依赖。实际注册/发现/调用往返和256存储边界必须验证。 |

## Signature decision evidence

- 固定upstream源码：[messages.rs@2bdd1d6](https://github.com/xai-org/grok-build/blob/2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8/crates/codegen/xai-grok-sampler/src/stream/messages.rs) 在BlockState增signature_delta_seen，首delta清seed再push、后push。确认差异真实，不等于确认Grow接入路由使用该扩展。
- Grow `sampler/src/stream/messages.rs` 的ContentBlock::Thinking初始化保存signature；StreamDelta::SignatureDelta赋值。现有“start缺签名、稍后delta补齐”契约和已有fixture仍成立。
- 官方[流协议说明](https://platform.claude.com/docs/en/build-with-claude/streaming)（本轮实际获取）只描述thinking block stop前特殊signature_delta；display omitted明确single delta。它未定义同block重复完整签名与多片段的判别，不据此声称禁止所有多delta。
- Context7实际查询官方[TypeScript MessageStream.ts](https://github.com/anthropics/anthropic-sdk-typescript/blob/main/src/lib/MessageStream.ts)返回 `signature: event.delta.signature`，相邻thinking使用append。官方[Python _messages.py](https://github.com/anthropics/anthropic-sdk-python/blob/main/src/anthropic/lib/streaming/_messages.py)已取得相同覆盖行为。SDK实现是当前语义的对照，非协议全部可能性的证明。

如果实际gateway提供同block语义分片，后续用保留原SSE帧边界的无凭据fixture区分start-only、seed+一delta、多delta分片、重复/完整快照、交错block及native回传验证，再确定是特定路由适配还是通用协议契约。不能凭手造多个delta拼接测试证明该流来自真实受支持路由，或把opaque签名随意改写成无效native continuation。

## Name boundary evidence

`mcp/src/servers.rs::into_registration` 在model visibility检查前对qualified server__tool执行provider名正则（64）；因此过长name连发现目录、app-only信息也被跳过。`shell/session/actor/mcp_snapshot.rs` 从完整bridge目录建立ToolIndex；`tools/implementations/search_tool`输出name/schema为JSON；`use_tool::dispatch_local_mcp`以ToolId交内部terminal。`common/tool-protocol/src/ids.rs` ToolId语法无64上限。

与此不同，`agent/src/agent.rs::tool_definitions`公开承诺“for sampling API”，委托ToolBridge完整目录；它不能在目录放宽后无意携带>64的MCP name给provider。`SessionActor::tool_definitions`当前从builtins_only建立普通live工具列表，不代表公开API的全部消费者。完整静态搜索没有发现更多仓内live direct消费者，也不能据此删掉该public seam或假定外部无调用方。

独立只读核验 `01a1b02c-58af-7122-87f6-104a0b96bd4e`确认上述目录/发现/内部调用边界；父随后亲读Cargo依赖，**Agent没有MCP依赖**。不能为使用 `mcp::servers::validate_tool_name` 给Agent新增MCP高层依赖。2026-11-07父再次按符号核验：仓内该函数只有MCP registration调用，Shell仅重导出，未找到另一个实际执行消费者；这不是外部API无人使用的证明。放宽后MCP只消费目录约束，Agent消费provider约束，两者已非同一校验。因此撤回“必须将纯校验移到Tools作为共享公共函数”的早期候选；当前最小方向是在Agent采样投影局部校验现64规则，MCP目录owner保256规则，两侧无需新增公共helper、alias或反向依赖。若后续发现两个真实provider投影消费者，再决定共同所有者，不为可能复用先建公共实体。公开Agent seam保留且返回provider合法定义，完整ToolBridge目录保持。

父也补查metadata snapshot填充：`refresh_mcp_snapshot_and_schedule_reminder_with`直接以完整ToolBridge.tool_definitions生成qualified_name/schema并发布eligibility；因此长名目录保留应沿原snapshot路径自动进入search与权限，不应另建catalog。app-only仍由register_mcp_tool前的model_visible门闩控制，放宽不得把它纳入模型可调的bridge。

最小实现边界应分别归MCP registration（目录identity合法、上限256，既有qualified分隔/语法不变）和Agent sampling projection（provider名仍64、长name仅可经meta工具）；无需截断、生成alias、复制新目录或替换权限来源。必须用同一长server/tool真实注册→search schema→use_tool原identity、以及公开Agent采样projection64/65/256/257控制来验收。这是 2026-11-07 的冻结分析结论；后续实现及验收见下方闭合记录。

2026-11-07补充官方边界：Context7实际读取[正式2026-07-28 tools规范](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/server/tools.mdx)，原server内tool name的SHOULD为1–128字符、区分大小写、ASCII字母/数字/underscore/hyphen/dot；聚合客户端SHOULD采用server identifier等消歧，serverInfo.name不保证唯一。返回资料另含旧SEP986的1–64建议，该提案不是同版正式工具契约，不能混成当前限制。**256是本候选的Grow qualified目录策略，不是官方MCP原始name上限；64是直接provider投影的现有跨provider约束。** 本次只放宽长度，既有ToolId/qualified唯一分隔等语法不变，官方可用dot不自动授权扩充当前产品ToolId字符集。

## Preserved prior decisions

Plan递延review已按原范围闭合；MCP Form/URL的产品交互尚未完成，不因这里的transport修复改变状态。统一Dock、链式model/window/effort仍为已拒绝方向；Memory/Remote Control仍延期。本轮没有因此启动这些功能，也未发布2.6.2。

## MCP name-boundary closure, 2026-11-09

Agent 64 字符直接 provider 投影与 MCP 256 字符完整目录身份已经分离。真实 Shell catalog 模块 17/17、Agent 504/MCP 200 回归和 descriptor 完整源码旧红新绿控制通过；原名、schema、app-only、禁用、exact permission 与 live-parent 资格保持。普通 descriptor 路径不变，先前不可写的最长 raw name 使用有界组件子目录。证据和跨平台限制见[最终验证](../2026-11-09-separate-mcp-catalog-name-bound/verification.md)。签名拼接的既有证据门槛不因此改变。
