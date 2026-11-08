# Design

## 决策依据

父复核`mcp::McpTool::into_registration`先parse唯一qualified边界，再用64字符provider正则决定是否进入目录。`ToolBridge::tool_definitions`返回完整目录；Shell MCP snapshot由该完整目录生成ToolIndex和eligibility，search_tool把name/schema放JSON值，use_tool用完整ToolId内部dispatch。`crates/common/tool-protocol/src/ids.rs`只校验语法、不设64长度。因此目录在registration就丢长名会损失这些现有能力，而放宽长度不需要复制目录或授权路径。

与此不同，公开`Agent::tool_definitions`明确承诺采样定义，现直接代理完整bridge；普通Shell采样的builtins-only不能替它承担契约。Agent无MCP依赖，给它反向依赖高层MCP只为校验会破坏分层。现`validate_tool_name`仓内只有MCP registration实际使用，Shell仅重导出；两侧未来分别消费不同长度规则，无必要公共共享helper。

官方正式MCP原始tool-name建议与256 qualified政策不是同一件事，固定来源见上游评审。当前选择只放宽Grow目录长度，保原首字符ASCII letter/underscore、其余ASCII alphanumeric/underscore/hyphen及唯一`__`边界；不顺手放行dot、colon、歧义或空段。

## 两个owner与实现顺序

1. **Agent sampling projection**先落实。在现`Agent::tool_definitions`取到bridge目录后，私有局部过滤现共同provider格式`^[a-zA-Z_][a-zA-Z0-9_-]{0,63}$`；schema、description、相对顺序原样保留。取值范围是该方法输出的所有function names，不通过另一遍动态目录读取猜namespace；原生builtin本已符合此格式，非法/过长custom name也不能被直接发给provider。该方法已有Vec返回型，省略不合格声明，不新增Result或capability registry。检查用现依赖或小的私有ASCII谓词，具体算法由coder选择，无新增公共API/dependency。完整bridge不改，meta-tool仍可找到长MCP identity。
2. **MCP registration**在1已接受后，只将现长度校验放宽至256，语法/qualified parse/model_visible保持；调整现validator注释与错误语义为Grow目录约束。保现公开函数，不为此另建公共provider validator或给Agent新增MCP依赖。错误/日志应准确写目录256，不能沿用“provider64”误导。permission、disabled与app-only gate由原owner继续决定。

不会截断、alias或把长名映射为hash。两个长名共享64前缀不能制造一个同名provider工具，完整identity仍唯一。256是完整qualified ASCII字符/字节数，不给server/tool分别各256，也不新增原始name的独立128强制门槛。

## Coder Task Specs / Review Plan

**Agent owner一项行为**：上述provider投影过滤，真实Agent public方法与其相同ToolBridge对照。目录内64/65/256三种qualified名称及共同64前缀的两个长名同时存在，公开projection只保64，bridge仍保全部；普通builtins/短MCP的schema、顺序保持。测试可以通过真实bridge注册小的既有Tool fixture，不需要MCP crate依赖；不能只单测谓词。scope为agent.rs/相邻Agent tests，无须改Tools公共面。

**MCP owner一项行为**：长度256 registration与其现调用链。64/65/256接纳、257拒绝，空段/多`__`/非法首字符/非ASCII和dot等原拒绝保持；app-only的accepted metadata不得进入modelcallable bridge。保name/schema原值，禁用/权限照常。

**跨层里程碑**由父在两者接受后核：同一长server/tool经真实MCP注册、Shell snapshot与search/schema、use_tool内部terminal，实际fake server收到原raw tool name和arguments；完整qualified identity不变。至少一个>64通常长度和256边界覆盖目录/路径实际消费者，不能只以ToolId接受宣称artifact路径也可用。若现消费者在256触发真实文件名/存储边界，带实际失败回报父，不能改名、截断或删256场景掩盖；父决定属于本次既有消费者适配还是独立缺口。modelvisibility与disabled/permission用现实际入口控制，主Agent public projection需同时保持64结果。

单一Rust/Cargo窗口串行，各coder只拥有当前模块承诺。MCP-F1/F2写同一邻区时先完成候选review与退役，不并发编辑。无需重跑无关GUI、全provider或全workspace；针对实际改变的public projection/registration/内部调用覆盖后即可结束。父亲读实际diff、计数/identity证据和产物限制，同步开发说明、上游候选状态与backlog，strict归档。当前只有已冻结设计，未计实现/验收。


## 256 字符的 descriptor 文件边界

实际 Linux 文件系统验证：raw name 250 字节加 `.json` 可写，253 字节加 `.json` 返回 `ENAMETOOLONG`。后者仍是合法 `s__<raw>` 256 字符目录身份。`McpClient::materialize_descriptors` 是公开的 best-effort helper；全仓搜索未发现运行期调用方，但不能因没有仓内调用方忽略公共存储入口。

保持全部既有可写 flat 路径；仅 sanitized name 超过 250 字节时写到 `tools/long-names/<完整 sanitized name>/descriptor.json`。合法目录身份的 raw name 最大 253 ASCII 字节，目录组件仍在 255 字节内。`long-names` 不以 `.json` 结尾，不会与 flat descriptor 文件冲突。文件仍由同目录 NamedTempFile 原子替换，重复发布不产生临时文件残留，整个 server directory 的树状清理自然包括新增子目录。sanitizer 继续去除路径分隔符；tool 的 wire name、qualified name 和 JSON name 都不改变。既有 sanitizer 同名碰撞策略及超出目录契约的任意上游名字不在本次扩张。
