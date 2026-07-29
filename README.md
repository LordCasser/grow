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
| 思考强度 | 依赖产品模型能力 | 每个 BYOK 模型显式声明 `reasoning_efforts`；`Shift+Tab` 按声明顺序循环 |
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
output_limit = 65536

[provider.zuozuo]
api_backend = "messages"

[provider.zuozuo.options]
base_url = "https://cyber.85466110.xyz/v1"
env_key = "ZUOZUO_API_KEY"

[provider.zuozuo.models.claude-opus-5]
name = "Claude Opus 5"
context_window = 200000
output_limit = 131072
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
[LLM Providers and BYOK](crates/codegen/grow-pager/docs/user-guide/11-custom-models.md)。

### 配置模型思考强度

不同供应商没有统一的模型能力发现接口。需要切换思考强度时，在具体模型下用
`reasoning_efforts` 显式声明该模型真正接受的档位；数组顺序就是 `Shift+Tab` 的循环顺序，
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
thinking/output-config 字段。`Shift+Tab`、`/effort` 和 `/model` 共用同一份模型档位配置。
切换结果保存在当前 session。有效默认值按精确程度解析：已有 session 的最后选择、模型上的
`reasoning_effort` 或 `default = true`、模型支持的全局 `default_reasoning_effort`，最后是该
模型声明档位中的最低值。没有声明 `reasoning_efforts` 的 BYOK 模型不会被 Grow 猜测支持，
按 `Shift+Tab` 时只会显示配置提示。

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

## 从源码构建

当前支持 macOS arm64、Linux arm64 和 Linux amd64。构建前需要：

- Git 和可用的 C/C++ 编译工具链（macOS 使用 Xcode Command Line Tools，Linux 使用对应
  发行版的基础构建工具）
- [Rustup](https://rustup.rs/)；进入仓库后会根据 `rust-toolchain.toml` 自动安装并使用固定
  的 Rust 工具链
- Protocol Buffers 编译器：推荐安装 [DotSlash](https://dotslash-cli.com) 以使用仓库中的
  `bin/protoc`；也可以通过 `$PROTOC` 指定本机 `protoc`，或确保 `protoc` 已在 `PATH` 中
- [ripgrep](https://github.com/BurntSushi/ripgrep)：源码构建默认直接使用 `PATH` 中的 `rg`；
  GitHub Release 产物已经内嵌固定版本，不要求终端用户另行安装

```sh
git clone https://github.com/LordCasser/grow.git
cd grow

# 使用仓库内固定版本的 protoc
cargo install dotslash

# 编译并直接启动开发版本
cargo run --locked -p grow-pager-bin --bin grow
```

构建优化后的本机二进制：

```sh
cargo build --locked --release -p grow-pager-bin --bin grow
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
cargo check --locked -p grow-pager-bin
```

Cargo build script 不会联网下载 ripgrep。如果希望本地构建一个不依赖系统 `rg` 的自包含
二进制，先用 `command -v rg` 找到本机绝对路径，再显式提供给构建：

```sh
env GROW_TOOLS_BUNDLE_RG_PATH=/absolute/path/to/rg \
  cargo build --locked --release -p grow-pager-bin --bin grow
```

官方 GitHub Release 使用更激进的 `release-dist` profile。复现同样的自包含构建时也要
显式提供目标平台的 `rg`：

```sh
env GROW_TOOLS_BUNDLE_RG_PATH=/absolute/path/to/rg \
  cargo build --locked --profile release-dist -p grow-pager-bin --bin grow
```

对应产物位于 `target/release-dist/grow`。

当前官方 release 只构建 macOS arm64、Linux arm64 和 Linux amd64。创建并发布
`v<crate-version>` GitHub Release 后，[release workflow](.github/workflows/release.yml) 会构建
这三个目标。CI 会下载并校验与目标架构匹配的固定版本 ripgrep，确认该二进制可通过绝对
路径在没有 `PATH` 的环境中运行后将其嵌入 Grow，最后上传版本化资产与 `SHA256SUMS`。

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
- `Shift+Tab`：按当前模型的 `reasoning_efforts` 顺序循环思考强度
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
