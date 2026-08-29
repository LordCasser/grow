<p align="center">
  <img src="grow.png" alt="Grow">
</p>

# Grow

Grow 是 Rust 终端 AI 编程 Agent。它提供三种交互入口：交互式 TUI、无界面的 headless 命令，以及面向编辑器和客户端的 ACP 服务。

Grow 采用 BYOK：模型、Provider、凭据和路由都由用户配置。会话与诊断默认保存在本地。Grow 不是 xAI 官方产品，也不内置模型、账号或推理端点。

[Releases](https://github.com/LordCasser/grow/releases) · [快速开始](#快速开始) ·
[User Guide](crates/codegen/pager/docs/user-guide/README.md) ·
[config.example.toml](config.example.toml)

## 安装

### 下载 Release

从 [Latest GitHub Release](https://github.com/LordCasser/grow/releases/latest) 下载与你的系统匹配的归档。支持 macOS、Linux、Windows 的 x86_64/arm64；Linux 另支持 riscv64，OpenHarmony 支持 arm64。

每个归档只含一个可执行文件：Unix 是 `grow`，Windows 是 `grow.exe`。解压后把它放入
`PATH` 中即可。

下面用 GitHub CLI 演示 macOS Apple Silicon；其他平台只需替换归档 pattern：

```sh
GROW_DOWNLOAD_DIR="$(mktemp -d)"
cd "$GROW_DOWNLOAD_DIR"
gh release download --repo LordCasser/grow \
  --pattern 'grow-*-macos-aarch64.tar.gz' \
  --pattern SHA256SUMS
shasum -a 256 --check SHA256SUMS --ignore-missing
tar -xzf grow-*-macos-aarch64.tar.gz
mkdir -p "$HOME/.local/bin"
install -m 0755 grow "$HOME/.local/bin/grow"
```

请在同一个 Release 中取得归档和 `SHA256SUMS`，并按实际文件名核对 SHA-256；校验用于发现
传输损坏或归档被替换，不等同于独立的发布者签名。

### 从源码构建

见[开发](#开发与许可证)。

## 快速开始

Grow 不猜测模型。首次启动前，在 `$GROW_HOME/config.toml`（默认
`~/.grow/config.toml`）中配置一个 Provider、一个模型和 `[models].default`：

```toml
[models]
default = "example/my-model"

[provider.example]
api_backend = "chat_completions"

[provider.example.options]
base_url = "https://api.example.com/v1"
env_key = "EXAMPLE_API_KEY"

[provider.example.models.my-model]
name = "Example Model"
context_window = 128000
```

然后设置凭据并启动：

```sh
export EXAMPLE_API_KEY="your-key"
grow
```

`api_backend` 是协议选择，不是厂商选择：`chat_completions` 使用 Chat Completions，
`responses` 使用 Responses，`messages` 使用 Anthropic-compatible Messages。完整的
Provider、模型字段、环境变量和本地端点示例见 [Authentication](crates/codegen/pager/docs/user-guide/02-authentication.md)
与 [Custom Models](crates/codegen/pager/docs/user-guide/11-custom-models.md)。

## 日常使用

### 常用命令

```sh
grow                                  # 进入 TUI
grow "修复失败测试并运行相关用例"        # 启动并提交首个任务
grow --cwd ~/projects/my-app          # 指定工作目录
grow --worktree=feature "实现新功能"    # 在 git worktree 中执行
grow -m example/my-model              # 选择已配置模型
grow -c                                # 继续最近的 session
grow --resume SESSION_ID               # 恢复指定 session
grow -p "检查当前仓库并输出结论"          # headless 执行
```

无界面输出和脚本用法见 [Headless Mode](crates/codegen/pager/docs/user-guide/14-headless-mode.md)。
ACP 的 stdio 与 WebSocket 服务见 [Agent Mode](crates/codegen/pager/docs/user-guide/15-agent-mode.md)。

### TUI 控制入口

`Ctrl+X` 是会话控制入口的前导键：

| 按键 | 作用 | 对应 slash |
| --- | --- | --- |
| `Ctrl+X`，`M` | 选择模型 | `/model` |
| `Ctrl+X`，`A` | 选择主 Agent | `/agent` |
| `Ctrl+X`，`E` | 选择 reasoning effort | `/effort` |
| `Ctrl+X`，`P` | 选择 Permission | `/permission` |
| `Ctrl+X`，`B` | 选择 Behavior | `/behavior` |

Agent 是角色与任务边界，Behavior 是主会话的推进方式，Permission 是工具调用的授权策略。
三者独立：切换其中一个不会暗中改动另外两个。完整按键与命令见
[Shortcuts](crates/codegen/pager/docs/user-guide/03-keyboard-shortcuts.md) 和
[Slash Commands](crates/codegen/pager/docs/user-guide/04-slash-commands.md)。

## Workflow 与 Goal

### Workflow

Workflow 用来把可复用的多步任务交给确定性的编排脚本：

- `/workflow [prompt]` 进入 Workflow Behavior；也可以用 `/behavior workflow`。
- 自然语言“执行 xxx workflow”会先按 `name`、`description`、`when_to_use` 搜索：唯一匹配直接使用，
  有歧义时询问；无匹配时引导创建当前 session 的临时草稿。
- `/workflow-run` 打开运行选择器。
- `/workflow-run <name> [args]` 启动已注册 Definition；已保存 Definition 也可用动态 `/<name> [args]`。
- `/workflows` 管理 Definitions 和 Runs。

Workflow Definition 是 Rhai 脚本。编辑、验证、发布和运行见
[Workflow Workspace](docs/architecture/workflow-workspace.md) 与
[Workflow Rhai](docs/workflow-rhai.md)。

### Goal

Goal 保存一个可编辑、可暂停和可恢复的长期目标。常用命令为：

```text
/goal set <objective>
/goal status
/goal pause
/goal restart
/goal clear
```

目标处于 Active 且会话空闲时，Grow 会继续推进它；用户输入和待处理通知优先。Paused、Blocked、BudgetLimited、Complete 状态会停止自动续跑。

## 核心技术

- **Append-only Timeline**：会话事实只追加；模型上下文、会话 transcript 与 Trajectory 都由同一
  Timeline 投影出 Surface（当前有效上下文），不各自维护另一份历史。
- **可恢复生命周期**：Turn、Step、Request、Tool 都有显式开始与终态；崩溃或取消时沿同一生命周期
  补齐可审计的恢复结果。
- **压缩只改变投影**：上下文压缩替换当前 Surface 的可见范围，原始事件仍保留，可用于恢复、检索和
  Trajectory；它不会删除会话历史。
- **独立控制域**：Model/effort 与 Agent 在 Step 边界切换，Behavior 决定后续 Turn 的推进协议，
  Permission 只决定已通过 Agent 和运行时能力约束的调用是否获批，彼此不替代。
- **独立子会话**：subagent 使用独立 child session，有自己的生命周期和能力边界；父会话只接收其
  明确的结果。
- **确定性 Workflow**：Rhai Definition 按确定性规则执行；每个 Run 固定 Definition 内容与启动
  参数的不可变快照，修改 Definition 不会改变已启动的 Run。

## 扩展、配置与安全

### 扩展入口

- [MCP Servers](crates/codegen/pager/docs/user-guide/07-mcp-servers.md)：连接外部工具和服务。
- [Skills](crates/codegen/pager/docs/user-guide/08-skills.md)：复用提示和工作方法。
- [Plugins](crates/codegen/pager/docs/user-guide/09-plugins.md)：打包 Skills、Commands、Agents、Hooks 和 MCP。
- [Hooks](crates/codegen/pager/docs/user-guide/10-hooks.md)：在工具调用前后执行检查或回调。
- [AGENTS.md 与项目规则](crates/codegen/pager/docs/user-guide/12-project-rules.md)：为目录提供项目约束。
- [Subagents](crates/codegen/pager/docs/user-guide/16-subagents.md)：运行独立的子 Agent 会话。

### 配置位置

主配置是 `$GROW_HOME/config.toml`，默认路径为 `~/.grow/config.toml`，用于个人的 Provider、
模型、认证、界面和会话设置。项目 `.grow/config.toml` 只在该项目范围内生效，用于项目侧的
MCP、Plugin、Permission 与相关资源配置。

### Permission 与 Sandbox

Permission 有三种会话模式：

- `ask`：需要时询问用户，适合交互式使用。
- `auto`：对工具调用进行自动安全判断，无法自动放行时阻止或升级处理。
- `always-approve`：通常不等待交互确认，适合已明确边界的自动化；`deny` 规则、Hooks 等硬限制仍生效。

allow/ask/deny 规则可以按工具、命令或路径细化策略；Hooks 可以在工具执行前拒绝调用。Sandbox
进一步提供操作系统级的文件系统和网络隔离。详见 [Permissions and Safety](crates/codegen/pager/docs/user-guide/22-permissions-and-safety.md)
与 [Sandbox](crates/codegen/pager/docs/user-guide/18-sandbox.md)。

### 数据与网络边界

- 模型调用走用户配置的 Provider/model routes；Grow 不提供默认模型或默认推理端点。
- MCP、Plugin 和更新检查在对应功能使用时可能联网。
- Grow 不主动上传 session 或 diagnostics；会话、状态和本地诊断留在本地文件中。

## 文档导航

- 入门与完整索引：[User Guide](crates/codegen/pager/docs/user-guide/README.md)。
- 接入模型：[Authentication](crates/codegen/pager/docs/user-guide/02-authentication.md) ·
  [Custom Models](crates/codegen/pager/docs/user-guide/11-custom-models.md)。
- 操作参考：[Shortcuts](crates/codegen/pager/docs/user-guide/03-keyboard-shortcuts.md) ·
  [Slash Commands](crates/codegen/pager/docs/user-guide/04-slash-commands.md)。
- 会话与安全：[Sessions](crates/codegen/pager/docs/user-guide/17-sessions.md) ·
  [Permissions](crates/codegen/pager/docs/user-guide/22-permissions-and-safety.md) ·
  [Sandbox](crates/codegen/pager/docs/user-guide/18-sandbox.md)。
- 自动化与集成：[Headless](crates/codegen/pager/docs/user-guide/14-headless-mode.md) ·
  [ACP](crates/codegen/pager/docs/user-guide/15-agent-mode.md)。
- 架构：[Agent Core Timeline](docs/architecture/agent-core-timeline.md) ·
  [Behavior State](docs/architecture/behavior-state-overview.md) ·
  [Goal Continuation](docs/architecture/goal-continuation.md) ·
  [Workflow Workspace](docs/architecture/workflow-workspace.md) ·
  [Prompt Architecture](crates/codegen/agent/PROMPT_ARCHITECTURE.md)。

## 开发与许可证

### Prerequisites

- Git
- [Rustup](https://rustup.rs/) 与仓库声明的 Rust toolchain
- 本机 C/C++ 基础构建工具
- [ripgrep](https://github.com/BurntSushi/ripgrep)

### 构建与测试

```sh
git clone https://github.com/LordCasser/grow.git
cd grow

cargo check --locked -p cli
cargo test --locked -p workspace --lib
cargo test --locked -p shell --lib
cargo test --locked -p pager --lib
cargo build --locked --release -p cli --bin grow
```

源码入口和模块边界见仓库目录及上述架构文档。Grow 的第一方代码使用 Apache License 2.0，
见 [LICENSE](LICENSE)。第三方和 vendored 代码沿用各自许可证，见
[THIRD-PARTY-NOTICES](THIRD-PARTY-NOTICES) 与 [third_party/NOTICE](third_party/NOTICE)。

Grow fork 自 xAI Grok Build 的开源代码，但不是 xAI 官方产品。
