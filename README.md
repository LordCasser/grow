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
| 推理凭据 | 可依赖产品登录态 | 每个 provider 自带 API key、环境变量、auth helper 或显式 OAuth；不存在全局内置推理登录态 |
| 默认模型 | 产品默认值 | `[models].default` 只为新 session 提供初始值 |
| session 恢复 | 受产品模型状态影响 | 打开已有 session 时恢复该 session 上次退出时的 Agent 和模型 |
| 模型切换 | slash command / 旧式选择流程 | `Ctrl+X` 后按 `m` 打开已配置模型选单，同时保留 `/model` |
| Agent 切换 | 上游内置 Agent 流程 | `Ctrl+X` 后按 `a` 打开平级 Agent 选单，同时保留 `/agents` |
| Agent 定义 | 上游格式 | 兼容 Harness/OpenCode 常见 Markdown frontmatter；忽略外部权限、mode 和 model 字段 |
| Agent 关系 | 可包含产品特定模式 | 所有 Agent 平级；Agent 不绑定 provider/model，也不引入父/子 Agent 限定 |
| 权限切换 | 上游快捷键/模式 | `Ctrl+R` 循环 Grow 原有权限模式；Tab 保持原行为 |
| Agent/Skill 用户目录 | 产品目录 | 优先 `~/.config/.grow`，同名资源找不到时再回退 `~/.agent` |
| 缺少 LLM 配置 | 可落入产品默认模型 | 连接前阻止启动，并引导编辑 `~/.grow/config.toml` |

Grow 的 ACP 扩展协议使用 `grow/*`（转发层使用 `_grow/*`）。上游名称只保留在来源说明、
许可证和历史 changelog 中，不再作为运行时服务或协议命名。

## 网络边界

Grow 没有内置的 xAI/Grok 服务端点、OAuth issuer 或 OAuth client。未在本地显式开启
`[cli].auto_update = true` 时，也不会在后台检查 GitHub Release 更新。

运行时连接分为三类：

- 模型调用：只访问当前 session 所选 provider 的 `base_url`。
- 用户显式配置：MCP、插件源、外部 OTLP、反馈、trace upload 和可选服务端点只在用户配置
  或执行对应操作后访问；远程拉取、语音、图像和视频能力默认关闭。
- `grow/*` 与 `_grow/*` 是本地 ACP wire protocol，不表示任何外部服务。

内部 OTLP 只有在本地开启 telemetry 且设置 `GROW_INTERNAL_OTLP_TRACES_ENDPOINT` 时才导出；
它不会从聊天代理派生端点，也不会读取标准 `OTEL_EXPORTER_OTLP_*`。标准 `OTEL_*` 仅供用户
显式启用的 external OTLP 流使用。

## 配置 LLM（BYOK）

首次启动前，在 `~/.grow/config.toml` 中至少声明一个 provider/model，并设置全局默认：

```toml
[models]
default = "zuozuo/claude-opus-5"

[provider.zuozuo]
api_backend = "messages"

[provider.zuozuo.options]
base_url = "https://cyber.85466110.xyz/v1"
env_key = "ZUOZUO_API_KEY"

[provider.zuozuo.models.claude-opus-5]
name = "Claude Opus 5"
context_window = 200000
```

`api_backend` 描述端点协议，而不是厂商名称：

- `chat_completions` → `/v1/chat/completions`
- `responses` → `/v1/responses`
- `messages` → `/v1/messages`

也可以在 `[provider.<id>.options]` 中使用 `api_key`，但推荐通过 `env_key` 从环境变量读取。
完整字段和 session 继承规则见
[LLM Providers and BYOK](crates/codegen/grow-pager/docs/user-guide/11-custom-models.md)。

OAuth 是 provider 的另一种可选凭据来源，不是 Grow 的全局登录态。模型仍然必须显式属于
该 provider，也不会因为登录成功而自动添加模型：

```toml
[models]
default = "example/model-a"

[provider.example]
api_backend = "responses"

[provider.example.options]
base_url = "https://api.example.com/v1"

[provider.example.options.auth]
type = "oauth"
issuer = "https://auth.example.com"
client_id = "public-client-id"
scopes = ["openid", "profile", "offline_access"]

[provider.example.models.model-a]
name = "Model A"
```

使用 `grow login example` 登录，使用 `grow logout example` 只清除这个 provider 的凭据。
只有一个 OAuth provider 时可以省略名称。API key / `env_key` 仍优先于 OAuth，因此 BYOK
保持默认且无需登录。

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

用户级资源按以下顺序加载：

1. `~/.config/.grow/agents/`、`~/.config/.grow/skills/`
2. 对当前名称未命中时，回退到 `~/.agent/agents/`、`~/.agent/skills/`

项目级 Agent 可以放在 `.grow/agents/`。Agent 使用 Markdown + YAML frontmatter；权限模式、
provider 和 model 都属于 session，不属于 Agent 定义。格式见
[grow-agent README](crates/codegen/grow-agent/README.md)。

## 构建与运行

要求：

- Rust 工具链（由 `rust-toolchain.toml` 固定）
- [DotSlash](https://dotslash-cli.com)，用于运行 `bin/` 下的 hermetic 工具
- `protoc`（优先通过 `bin/protoc` + DotSlash 解析，也可使用 `$PROTOC` / `PATH`）

```sh
cargo run -p grow-pager-bin --bin grow
cargo build -p grow-pager-bin --release
cargo check -p grow-pager-bin
```

release 二进制位于 `target/release/grow`。

当前官方 release 只构建 macOS arm64、Linux arm64 和 Linux amd64。创建并发布
`v<crate-version>` GitHub Release 后，[release workflow](.github/workflows/release.yml) 会构建
这三个目标并上传 updater 所需的版本化资产与 `SHA256SUMS`。

自动更新只读取 [`LordCasser/grow` Releases](https://github.com/LordCasser/grow/releases)，且默认关闭：

```toml
[cli]
auto_update = true
```

Grow 不发布 npm 包，也不调用 npm 查询或安装更新；GitHub Release 中的原生二进制是唯一
分发渠道。

## 常用交互

- `Ctrl+X`，然后 `m`：选择已经配置的模型
- `Ctrl+X`，然后 `a`：选择 Agent
- `Ctrl+R`：循环权限模式
- `/model`、`/agents`：保留原命令行为
- `Tab`：保留原有补全/导航行为

## 仓库结构

| 路径 | 职责 |
|---|---|
| `crates/codegen/grow-pager-bin` | composition root，构建 `grow` 二进制 |
| `crates/codegen/grow-pager` | TUI、输入、modal、session UI |
| `crates/codegen/grow-shell` | Agent runtime、stdio/headless/leader 入口 |
| `crates/codegen/grow-agent` | Agent 定义、发现、prompt 与工具装配 |
| `crates/codegen/grow-tools` | 终端、文件、搜索等工具实现 |
| `crates/codegen/grow-config` | `GROW_HOME`、配置加载和路径约束 |
| `crates/codegen/grow-models` | 空的编译期模型目录；运行时模型只来自用户配置 |
| `crates/codegen/grow-workspace` | 文件系统、VCS、执行、权限和 checkpoint |
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
