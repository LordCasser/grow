# Grow

Grow 是一个从 xAI **Grok Build** 分叉而来的终端 AI 编程 Agent。它保留了上游成熟的
Rust TUI、Agent runtime、工具系统和 ACP 能力，但产品身份、模型配置和 Agent 交互已经
独立演进。

Grow 不是 xAI 官方产品，也不内置 Grok 模型、推理端点或推理凭据。所有 LLM 都由用户
通过 BYOK 配置提供。

## Grow 相对 Grok 的主要变化

| 领域 | Grok Build 上游 | Grow |
|---|---|---|
| 产品身份 | `grok`、`GROK_HOME`、`~/.grok`、`xai-grok-*` | `grow`、`GROW_HOME`、`~/.grow`、`grow-*` |
| 模型来源 | 带有产品内置模型和远端模型目录 | 仅加载用户声明的 `[provider]` 模型，不提供内置目录 |
| 推理凭据 | 可依赖产品登录态 | 仅支持用户提供的 API key、环境变量或本地密钥 helper；不存在 OAuth 或全局推理登录态 |
| 默认模型 | 产品默认值 | `[models].default` 只为新 session 提供初始值 |
| session 恢复 | 受产品模型状态影响 | 打开已有 session 时恢复该 session 上次退出时的 Agent 和模型 |
| 模型切换 | slash command / 旧式选择流程 | `Ctrl+X` 后按 `M` 打开已配置模型选单，同时保留 `/model` |
| 思考强度 | 依赖产品模型能力 | 每个 BYOK 模型显式声明 `reasoning_efforts`；`Ctrl+X` 后按 `E` 打开选单 |
| Agent 切换 | 上游内置 Agent 流程 | `Ctrl+X` 后按 `A` 打开平级 Agent 选单，同时保留 `/agent` |
| Agent 定义 | 上游格式 | 使用严格的 Grow Markdown frontmatter；未知字段以及 provider/model/mode/permission 字段均拒绝解析 |
| Agent 关系 | 可包含产品特定模式 | 所有 Agent 定义平级；主/子只是一次 session 中的运行角色，主 Agent 可通过工具策略限制本次可调用的子 Agent |
| 权限切换 | 上游快捷键/模式 | `Ctrl+X` 后按 `P` 打开 Ask / Auto / Always Approve 选单；`Ctrl+R` 用于 redo |
| Behavior 切换 | 与权限或 Agent 混合 | `Ctrl+X` 后按 `B` 打开 Normal / Clarify / Plan / Static Workflow / Deep Research / Goal 选单 |
| Agent/Skill 用户目录 | 产品目录 | 仅 `~/.grow`（项目级 `.grow` + 用户级 `~/.grow`） |
| 缺少 LLM 配置 | 可落入产品默认模型 | 连接前阻止启动，并引导编辑 `~/.grow/config.toml` |

Grow 的 ACP 扩展协议使用 `grow/*`（转发层使用 `_grow/*`）。上游名称只保留在来源说明、
许可证和历史 changelog 中，不再作为运行时服务或协议命名。

## 1.0.0 里程碑

`1.0.0` 是 Grow 脱离 Grok 产品运行时后的第一个里程碑版本。Grow 的第一方 crate 共享同一
workspace 版本，发布 tag 必须使用 `v1.0.0` 并与 `cli` 的 Cargo 版本严格一致。

这个里程碑明确收敛到代码和计算机任务：BYOK、多 Provider、Agent/Skill、MCP、LSP、Shell、
文件工具、browser/computer、视觉输入和本地 Session 是核心能力；遥测上传、计费/订阅、托管
Web Search、远程会话同步、远程公告以及图片/视频生成不属于 Grow 1.0.0。诊断信息只落到本地，
Web Search 由用户配置 MCP Server 提供。

准备 release 时：

1. 确认 `cargo metadata --locked --no-deps` 中所有 Grow 第一方包均为 `1.0.0`。
2. 按本文的 release 构建方式验证四个支持目标，并确认产物内嵌 `rg`。
3. 创建并推送 `v1.0.0` tag；不要预先公开 Release。workflow 会校验 tag，构建并验证四个平台的
   `grow` 二进制，在 draft Release 中上传完整资产后再一次性公开。

完整可复制配置见 [config.example.toml](config.example.toml)。示例不包含真实密钥，默认通过
`env_key` 读取环境变量。

## 架构边界

Grow 不把“当前用什么 Agent”“如何推进任务”“允许执行什么操作”混成一个 mode。主 Agent
的系统提示词按固定顺序组合：

```text
Mandatory Core → Audience → Agent Role → Active Behavior → Runtime Context
```

- **Mandatory Core** 始终存在，规定指令优先级、安全边界、工具规则和项目指令作用域。
- **Audience** 只区分直接面对用户的主 Agent 与接受委派的子 Agent。
- **Agent Role** 来自内置或用户 Markdown 定义，描述职责和工具策略。`promptComposition: full`
  只能替换标准 Role 基础，不会覆盖 Mandatory Core、Audience、Behavior 或 Runtime Context。
- **Behavior** 是主 Agent 推进当前目标的互斥协作协议。
- **Runtime Context** 注入当前工作区、会话和已解析能力等运行时事实。

最终工具能力是 Tool Registry、Agent policy、子 Agent 深度/能力限制、Behavior gate 和当前
Permission 决策的交集。Behavior 不会授予工具，Permission 也不会改变 Agent Role。
完整组合规则见 [Prompt Architecture](crates/codegen/agent/PROMPT_ARCHITECTURE.md)。

### Crate 边界原则

Grow 只在存在独立复用契约、依赖倒置、可部署产物或明确编译隔离收益时建立 crate；其余实现归入
拥有该行为的模块，不用“一类一个 crate”模拟架构。组合根是 `cli`，交互层属于
`pager`，Agent/session runtime 属于 `shell`，主机文件系统与工作区状态属于
`workspace`，工具协议与执行属于 `tools` 及其轻量 contract crate。

跨进程协议也遵循同一标准：只有实际存在独立进程、稳定 wire contract 或多语言消费者时才引入
protobuf/gRPC。纯 Rust 进程内边界直接使用 Rust 类型，需要落盘或外部交换时使用已有 Serde
协议。workspace 依赖必须有生产代码或测试消费者；失去消费者的生成器、转换层和依赖应一并删除。
仅由一个 runtime 消费的远程载荷类型归该 runtime 的 owner 模块，不为已经不存在的服务保留
`prod` 目录或独立 contract crate。
目前保留的单消费者 crate 仅限有明确收益的边界：`workflow` 提供独立执行语义，`memory`
隔离存储与向量依赖，`mermaid` 隔离不可信渲染和 vendored layout 栈，`pager-render` 隔离
大体量渲染编译单元。单一调用方本身不是建立 crate 的理由。

### Behavior、Role 与运行实例

主 Agent 在一个 session 中只处于一种 Behavior：
`Normal | Clarify | Plan(phase) | Static Workflow | Deep Research | Goal`。子 Agent 不继承这些
Behavior，只接收明确的 Role、任务和能力边界，也不能递归启动 Workflow。

| 概念 | 负责什么 | 不负责什么 |
|---|---|---|
| Behavior | 主 Agent 与用户如何推进目标 | 工具授权、Permission、子 Agent 职责 |
| Role | Agent 或子 Agent 的职责与策略 | 会话协作协议和运行生命周期 |
| WorkflowRun | 动态子计划的执行、journal、暂停与恢复 | 选择或改变当前 Behavior |
| GoalTracker | 目标、续跑状态和独立验证证据 | Agent Role 与 Permission |
| Permission | Ask / Auto / Always Approve 的审批策略 | 规划方式和工具注册 |

六种 Behavior 的边界是：Normal 直接完成当前请求；Clarify 持续询问会实质影响结果的未知
信息；Plan 先形成完整方案、等待人类批准，再严格执行冻结方案；Static Workflow 由主 Agent
分阶段推进——每阶段侦察后编写一个确定性 Rhai 工作流脚本并启动（至多一个运行，运行内可
`parallel()` 并行扇出子 Agent），然后 yield 等待完成通知，再根据结果决定下一阶段，不设
整体审批点；Deep Research 严格只读、交叉验证证据并保证交付终态报告；Goal 持续推进明确
目标，只有独立 verifier 判定 `Achieved` 才能完成。

Plan 的阶段固定为 `Drafting → AwaitingApproval → Executing`，重大偏离进入 `Amending` 并
重新审批。只有 Executing 可修改工作区；`plan_control` 是唯一生命周期接口。Plan 期间不会
暴露 Workflow tool。Workflow、Deep Research 和 Goal 的运行状态分别属于 Workflow runtime
或 GoalTracker，不会再叠加成隐藏 Behavior。

### 统一的会话交互

Model、Agent、Effort、Permission 和 Behavior 共享两种会话入口：Slash 选择器与 `Ctrl+X`
前导键。对应快捷键为 `M`、`A`、`E`、`P`、`B`；弹窗、补全、availability 和执行逻辑使用
同一份目录。Behavior 与 Permission 还提供 `/plan`、`/goal`、`/ask`、`/always-approve` 等
幂等一级命令。带任务文本的 Behavior 命令只有在切换真正 `Applied` 后才发送文本，避免把
目标交给旧 Behavior。

输入框右下角固定显示 `model | behavior | permission`，未选择特殊 Behavior 时显示
`normal`。设置菜单只保存新 session 的持久默认值；`/model`、`/permission`、`/behavior`
和前导键只修改当前 session。`/agent` 是 Agent/Behavior 两阶段选择器，`/agents` 打开运行
详情面板，Dashboard 只为下一次 spawn 暂存选择，不维护另一套配置协议。

### 从产品运行时收敛为本地 Agent

Fork 后删除的不是表面入口，而是整条产品依赖链：托管模型目录与推理登录态、计费/订阅、
遥测和 trace 上传、托管 Web Search、远程会话同步、远程公告、Coding Data Sharing、桌面
产品残留，以及图片/视频生成和远程 computer runtime。Grow 保留的是本地 session、代码与
计算机工具、ACP、MCP、LSP、插件、Agent/Skill 和用户显式配置的网络能力。删除范围构成架构
边界：这些产品服务不会以隐藏默认值、兼容别名或备用端点重新进入运行时。

## 网络边界

Grow 没有内置的 xAI/Grok 服务端点，也不实现 OAuth、OIDC、设备登录或 refresh token
生命周期。未在本地显式开启
`[cli].auto_update = true` 时，也不会在后台检查 GitHub Release 更新。

运行时连接分为三类：

- 模型调用：只访问当前 session 所选 provider 的 `base_url`。
- 用户显式配置：MCP、插件源和其他可选服务端点只在用户配置或执行对应操作后访问。
- 发布更新：仅在 `[cli].auto_update = true` 时访问 Grow 的 GitHub Releases。

Grow 不内置 Web Search 提供商。需要联网搜索时，请配置提供搜索能力的 MCP Server；
Grow 会像发现其他 MCP 工具一样发现它，不要求固定的服务器名或工具名。内置 `web_fetch`
仍可用于读取已知 URL。

- `grow/*` 与 `_grow/*` 是本地 ACP wire protocol，不表示任何外部服务。

Grow 不包含遥测、产品分析、Sentry、OTLP exporter 或 trace upload。诊断事件只写入用户指定
的本地日志；详见 [Local Diagnostics](crates/codegen/pager/docs/user-guide/24-monitoring-usage.md)。

## 配置 LLM（BYOK）

首次启动前，在 `~/.grow/config.toml` 中至少声明一个 provider/model，并设置全局默认：

```toml
[models]
default = "deepseek/deepseek-chat"
output_limit = 65536

[provider.deepseek]
api_backend = "chat_completions"

[provider.deepseek.options]
base_url = "https://api.deepseek.com/v1"
env_key = "DEEPSEEK_API_KEY"

[provider.deepseek.models.deepseek-chat]
name = "DeepSeek Chat"
context_window = 128000
output_limit = 65536
```

`api_backend` 描述端点协议，而不是厂商名称：

- `chat_completions` → `/v1/chat/completions`
- `responses` → `/v1/responses`
- `messages` → `/v1/messages`

也可以在 `[provider.<id>.options]` 中使用 `api_key`，但推荐通过 `env_key` 从环境变量读取。
`output_limit` 控制单次模型输出上限：模型级配置覆盖 `[models].output_limit`；两处都未配置时
Grow 不设置可选的输出限制。`context_window` 只用于本地上下文管理和自动压缩。请求时
`chat_completions`/`messages` 使用 `max_tokens`，`responses` 使用 `max_output_tokens`。
完整字段和 session 继承规则见
[LLM Providers and BYOK](crates/codegen/pager/docs/user-guide/11-custom-models.md)。
全部公开配置项的注释示例，以及多 Provider、模型思考强度、Agent/子 Agent、工具、权限、
MCP、Memory、本地公告和可替换 marketplace 的组合方式，见
[config.example.toml](config.example.toml)。

### 配置模型思考强度

不同供应商没有统一的模型能力发现接口。需要切换思考强度时，在具体模型下用
`reasoning_efforts` 显式声明该模型真正接受的档位；数组顺序就是 Effort 选单中的顺序，
`default = true` 标记新 session 的初始档位。以 DeepSeek 为例：

```toml
[models]
default = "deepseek/deepseek-v4-pro"
default_reasoning_effort = "max"

[provider.deepseek]
api_backend = "chat_completions"

[provider.deepseek.options]
base_url = "https://api.deepseek.com/v1"
env_key = "DEEPSEEK_API_KEY"

[provider.deepseek.models.deepseek-v4-pro]
name = "DeepSeek V4 Pro"
context_window = 1048576
reasoning_efforts = [
  { value = "high", label = "High", default = true },
  { value = "max", label = "Max" },
]
```

Grow 内部支持 `none`、`minimal`、`low`、`medium`、`high`、`xhigh` 和 `max`，但配置时只应
列出当前模型 API 实际支持的值。例如 [DeepSeek 思考模式](https://api-docs.deepseek.com/zh-cn/guides/thinking_mode/)
当前使用 `high`/`max`；[GLM 核心参数](https://docs.bigmodel.cn/cn/guide/start/concept-param)
允许模型按其接口能力声明更多档位。也可以使用简写：

```toml
reasoning_efforts = ["none", "high", "max"]
```

请求时，Grow 会根据 `api_backend` 转换字段：`chat_completions` 发送顶层
`reasoning_effort`，`responses` 发送 `reasoning.effort`，`messages` 使用对应的
thinking/output-config 字段。`Ctrl+X E`、`/effort` 和 `/model` 共用同一份模型档位配置。
切换结果保存在当前 session。有效默认值按精确程度解析：已有 session 的最后选择、模型上的
`reasoning_effort` 或 `default = true`、模型支持的全局 `default_reasoning_effort`，最后是该
模型声明档位中的最低值。没有声明 `reasoning_efforts` 的 BYOK 模型不会被 Grow 猜测支持，
此时 Effort 选单只会显示配置提示。

Grow 的认证边界是长期稳定的 BYOK-only：每个模型的凭据只能来自 `api_key`、`env_key`
或返回用户自有 API key 的本地 helper。Grow 不提供 `login/logout`，不打开授权页面，不保存
refresh token，也不替任何企业维护 OAuth/OIDC 会话。需要轮换密钥时，由环境、配置或 helper
在 Grow 外部完成。

本地 helper 只是读取 BYOK 的另一种方式：

```toml
[models]
default = "example/model-a"

[provider.example]
api_backend = "responses"

[provider.example.options]
base_url = "https://api.example.com/v1"
auth_provider = "company-secret-store"

[auth_provider.company-secret-store]
command = "/usr/local/bin/read-company-llm-key"
args = ["--format", "json"]
token_ttl_secs = 3600

[provider.example.models.model-a]
name = "Model A"
```

helper stdout 可以是裸 key，或 `{ "access_token": "...", "expires_in": 3600 }`。Grow 只在
内存中缓存结果；JSON 不接受 refresh token、issuer 或 client metadata。

## 插件市场启动源

Grow 不内置或识别 xAI 官方插件市场。需要自动加载并用于插件推荐的主市场时，在配置中
显式声明一个可替换的启动源；其他 `[[marketplace.sources]]` 仍作为普通附加源：

```toml
[marketplace.bootstrap]
name = "My Marketplace"
git = "https://github.com/example/plugin-marketplace.git"
branch = "main"

[[marketplace.sources]]
name = "Local Plugins"
path = "~/projects/plugins"
```

## Agent 与 Skill

Grow 只有一套平级的 Agent 定义。一个 Markdown 定义既可以被选作当前 session 的主 Agent，
也可以被其他 Agent 通过 `task` 工具启动为子 Agent；Agent 不绑定 provider/model/permission，
也不声明 mode、父 Agent 或子 Agent 身份。主/子只是运行时角色。

### 增加 Agent

新建一个 Markdown 文件即可增加 Agent。文件的相对路径是稳定 ID，例如
`.grow/agents/review/backend.md` 的 ID 是 `review/backend`；frontmatter 中的 `name` 只是兼容
元数据，不会改变 ID。

```text
<project>/.grow/agents/       # 当前项目，优先级最高
~/.grow/agents/               # Grow 用户级定义
```

项目级定义会从当前目录向上发现到仓库根。用户级资源只从 `~/.grow/agents/`、
`~/.grow/skills/` 加载，不再回退 `~/.agent`。

最小定义示例：

```markdown
---
description: Review code without modifying it
tools:
  - read_file
  - grep
  - list_dir
---

You are a strict code reviewer. Report concrete findings with file locations.
```

完整、可复制的所有字段见 [agent.md.example](agent.md.example)；字段语义见
[agent README](crates/codegen/agent/README.md)。权限模式、provider 和 model 都属于
session，不属于 Agent 定义。

### 选择、替换或停止使用主 Agent

在 `~/.grow/config.toml` 中按 ID 设置新 session 的默认主 Agent：

```toml
[agent]
name = "review/backend"
```

也可以直接指定一个 Markdown 文件。`definition` 比同一节中的 `name` 优先，通常二选一：

```toml
[agent]
definition = "/absolute/path/to/my-agent.md"
```

单次启动可使用 `grow --agent-profile /absolute/path/to/my-agent.md`。TUI 中按 `Ctrl+X` 后按
`a` 可切换当前 session 的主 Agent；选择会随 session 保存。新 session 使用全局默认，打开
已有 session 时优先恢复它上次使用的 Agent，因此修改 `[agent]` 不会改写已有 session。

主 Agent 始终必须存在，所以没有“关闭主 Agent”开关。停止使用某个自定义主 Agent 时：

- 把 `[agent].name` 或 `definition` 改成另一个定义，或删除 `[agent]` 回到内置默认
  `grow`。
- 删除或重命名对应 Markdown 文件，使它不再被发现。
- `[subagents.toggle]` 和 `/agents` 中的 Enabled 开关只控制它能否作为子 Agent 启动，
  不会禁止它被选择为主 Agent。

替换遵循发现优先级，而不是修改内置文件：

- 项目 `.grow/agents/<id>.md` 可同名覆盖项目内可见的用户定义或内置定义。例如创建
  `.grow/agents/explore.md` 可在该项目替换内置 `explore`。
- 为保证“列表中看到的定义就是实际调用的定义”，用户级文件不能同名覆盖内置 Agent；需要
  全局采用不同 ID，或用项目级文件覆盖。需要强制使用任意路径作为主 Agent 时使用
  `[agent].definition`。

新 session 的主 Agent 解析顺序为：ACP session 配置、`--agent-profile`、`[agent]`、
`GROW_AGENT`、内置 `grow`；恢复已有 session 时，已保存的 Agent 优先于这些全局
默认值。

### 增加、替换或禁用子 Agent

所有发现到的自定义 Agent 都可以作为子 Agent。内置可调用子 Agent 为
`general-purpose`、`explore` 和 `plan`。增加和替换仍使用上面的 Markdown 目录与同名覆盖
规则，不需要额外注册。

在配置中可以关闭某个子 Agent，省略的名称默认启用：

```toml
[subagents]
enabled = true

[subagents.toggle]
explore = true
plan = false
"review/backend" = true
```

包含 `/` 的 TOML key 必须像示例一样加引号。`/agents` 打开管理页面，
其中的 Enabled 开关写入同一份 `[subagents.toggle]`。要关闭整个子 Agent 系统，设置：

```toml
[subagents]
enabled = false
```

也可临时用 `GROW_SUBAGENTS=0` 强制关闭。全局关闭或 `[subagents.toggle]` 的禁用结果对所有
主 Agent 生效。

### 为不同主 Agent 指定可用子 Agent

在每个主 Agent 自己的 Markdown frontmatter 中，用 `tools` 里的 `Agent(...)` 限定它能看到
并实际启动的子 Agent。这是 task 工具权限，不会建立固定的父子层级：

如果不需要限制，主 Agent **不必声明 `Agent(...)`**。当 `tools` 整节未声明或为空时，它会
继承完整工具集，因此可以调用当前发现且全局启用的全部子 Agent：既包括内置的
`general-purpose`、`explore`、`plan`，也包括用户添加的 `review/backend` 等自定义 Agent。

例如下面的主 Agent 没有显式指定子 Agent，但仍可调用全部已启用的内置和用户子 Agent：

```markdown
---
description: General coordinator
---

Choose the most suitable subagent for each delegated task.
```

`tools` 的不同写法具有以下精确语义：

| 主 Agent 的 `tools` 配置 | 可用子 Agent |
| --- | --- |
| 未声明 `tools`，或 `tools: []` | 所有当前发现且全局启用的内置和用户子 Agent |
| 非空并包含裸 `Agent` 或 `task` | 所有当前发现且全局启用的内置和用户子 Agent |
| 非空并包含 `Agent(explore, review/backend)` | 仅列出的 `explore` 和 `review/backend` |
| 非空但不包含 `Agent(...)`、裸 `Agent` 或 `task` | 无；该主 Agent 不会获得 task 工具 |

```markdown
---
description: Coordinates review work
tools:
  - read_file
  - grep
  - Agent(explore, review/backend)
disallowedTools:
  - Agent(plan)
---

Delegate repository discovery to explore and code review to review/backend.
```

其中，带类型的 `Agent(...)` 会保留 task 工具及其生命周期工具，但只向模型展示并允许启动
列出的类型。`disallowedTools: [Agent(plan)]` 只拒绝指定类型；裸 `Agent` 则拒绝所有子 Agent。
`disallowedTools` 优先于 `tools`，全局 `[subagents.toggle]` 和 `[subagents].enabled` 仍是最终
上限，Agent 定义不能重新启用全局已禁用的类型。

例如，`coordinator.md` 可以只允许 `explore` 与 `review/backend`，而 `implementer.md` 可以只
允许 `general-purpose`。两者仍是平级定义，也都能被直接选为主 Agent。若子 Agent 自身允许
继续委派，同一规则会应用到它自己的定义，并同时受 `[subagents].max_depth` 限制。

“可用”表示该子 Agent 已进入 task 工具目录并通过运行时权限检查，不表示模型一定会选择它。
模型会依据任务内容以及 Agent 的名称和 `description` 自行决定委派对象；自定义 Agent 应提供
清晰、可区分的职责描述。需要确定性限制时，再使用 `Agent(...)` 白名单。

## 从源码构建

当前支持 macOS arm64、Linux arm64、Linux amd64 和 Linux riscv64。构建前需要：

- Git 和可用的 C/C++ 编译工具链（macOS 使用 Xcode Command Line Tools，Linux 使用对应
  发行版的基础构建工具）
- [Rustup](https://rustup.rs/)；进入仓库后会根据 `rust-toolchain.toml` 自动安装并使用固定
  的 Rust 工具链
- [ripgrep](https://github.com/BurntSushi/ripgrep)：源码构建默认直接使用 `PATH` 中的 `rg`；
  GitHub Release 产物已经内嵌固定版本，不要求终端用户另行安装

```sh
git clone https://github.com/LordCasser/grow.git
cd grow

# 编译并直接启动开发版本
cargo run --locked -p cli --bin grow
```

构建优化后的本机二进制：

```sh
cargo build --locked --release -p cli --bin grow
./target/release/grow --version
```

产物位于 `target/release/grow`。如需安装到用户路径：

```sh
mkdir -p "$HOME/.local/bin"
install -m 0755 target/release/grow "$HOME/.local/bin/grow"
```

确保 `$HOME/.local/bin` 已加入 `PATH`，之后可直接运行 `grow`。首次连接模型前还需要完成
上面的 [LLM（BYOK）配置](#配置-llmbyok)。只检查代码能否通过编译时，可运行：

```sh
cargo check --locked -p cli
```

Cargo build script 不会联网下载 ripgrep。如果希望本地构建一个不依赖系统 `rg` 的自包含
二进制，先用 `command -v rg` 找到本机绝对路径，再显式提供给构建：

```sh
env GROW_TOOLS_BUNDLE_RG_PATH=/absolute/path/to/rg \
  cargo build --locked --release -p cli --bin grow
```

官方 GitHub Release 使用更激进的 `release-dist` profile。复现同样的自包含构建时也要
显式提供目标平台的 `rg`：

```sh
env GROW_TOOLS_BUNDLE_RG_PATH=/absolute/path/to/rg \
  cargo build --locked --profile release-dist -p cli --bin grow
```

对应产物位于 `target/release-dist/grow`。

当前官方 release 构建四个目标：macOS arm64、Linux arm64、Linux amd64 和 Linux riscv64
（无 Windows / x86_64 macOS / 其他架构）。将 `v<crate-version>` tag 推送到
`LordCasser/grow` 后，[release workflow](.github/workflows/release.yml) 会构建这些目标；
Release 在四个 updater 约定资产上传并验证完成前保持 draft，避免自动更新看到尚未具备完整
下载项的新版本；带 semver 预发布段的版本会同步标记为 GitHub prerelease，避免 stable channel
选中 alpha 版本。Linux amd64/arm64 产物在
AlmaLinux 8 容器内以 `*-unknown-linux-gnu` 构建，glibc 基线 2.28（覆盖 RHEL 8 / Ubuntu
20.04+ / Debian 10+ 等），构建成功后对产物做版本烟雾测试（`grow --version`）；Linux
riscv64 在 amd64 runner 上通过 `cross` 交叉编译（官方无 riscv64 预编译 rg，
rg sidecar 由 CI 准备），产物不做本机烟雾测试；ripgrep sidecar 在构建前
staging 到工作区供容器使用。

GitHub Release 页面**只挂四个最终 `.tar.gz` 包**（`grow-{version}-linux-x86_64.tar.gz` /
`grow-{version}-linux-aarch64.tar.gz` / `grow-{version}-linux-riscv64.tar.gz` /
`grow-{version}-macos-aarch64.tar.gz`），每个包内仅包含名为 `grow` 的可执行文件，与 auto-update
契约一致；
ripgrep 下载包、Actions artifact 等只出现在 CI 过程中，不会作为 Release 下载项。
CI 会在构建时把固定版本 ripgrep 嵌入二进制，并跑 `grow --version` 烟雾测试。手动重跑已有
tag 时可以只修复 Linux 资产，但 `skip-macos` 要求对应 Release 已存在精确命名的 macOS 资产。

自动更新只读取 [`LordCasser/grow` Releases](https://github.com/LordCasser/grow/releases)，且默认关闭：

```toml
[cli]
auto_update = true
```

Grow 不发布 npm 包，也不调用 npm 查询或安装更新；GitHub Release 中的原生二进制是唯一
分发渠道。

## 常用交互

- `Ctrl+X`，然后 `M`：选择已经配置的模型
- `Ctrl+X`，然后 `A`：选择 Agent
- `Ctrl+X`，然后 `E`：选择当前模型的思考强度
- `Ctrl+X`，然后 `P`：选择 Permission
- `Ctrl+X`，然后 `B`：选择 Behavior；输入框右下角按 `model | behavior | permission` 显示当前状态
- `Ctrl+R`：重做输入框中的上一次撤销
- `/model`、`/agent`、`/effort`、`/permission`、`/behavior`：打开与快捷键相同的选单
- `/agents`：打开 Agent Dashboard；`/config-agents` 管理 Agent 定义
- `Tab`：保留原有补全/导航行为

## 仓库结构

| 路径 | 职责 |
|---|---|
| `crates/codegen/cli` | composition root，构建 `grow` 二进制 |
| `crates/codegen/pager` | TUI、输入、modal、session UI |
| `crates/codegen/shell` | Agent runtime、stdio/serve/leader 入口 |
| `crates/codegen/agent` | Agent 定义、发现、prompt 与工具装配 |
| `crates/codegen/tools` | 终端、文件、搜索等工具实现 |
| `crates/codegen/config` | `GROW_HOME`、配置加载和路径约束 |
| `crates/codegen/workspace` | 文件系统、VCS、执行、权限和 checkpoint |
| `crates/common/` | 通用叶子 crate |
| `third_party/` | vendored 第三方代码 |

根 `Cargo.toml` 由 workspace 生成流程维护；日常依赖调整优先修改各 crate 的
`Cargo.toml`。

## 开发

```sh
cargo check -p <crate>
cargo test -p <crate>
cargo clippy -p <crate>
cargo fmt --all
```

优先对受影响 crate 做针对性验证，完整 workspace 构建通常较慢。

## 来源与许可证

Grow 基于 xAI Grok Build 的开源代码分叉，`SOURCE_REV` 记录了所基于的上游提交。
上游历史保留在 changelog 和第三方声明中。

第一方代码使用 Apache License 2.0，见 [LICENSE](LICENSE)。第三方和 vendored 代码沿用
各自许可证，详见 [THIRD-PARTY-NOTICES](THIRD-PARTY-NOTICES) 和
[third_party/NOTICE](third_party/NOTICE)。
