## Context

现状依据：TaskToolInput/TaskTool 已公开 effort，但 `TaskTool::run` 丢弃 resume model/effort；`subagent/handle_request.rs::run_shell_child_inner` 再固定来源 route/effort/transport。`agent/handlers/model_switch.rs` 将用户选择拆成 enqueue/finish，使用 `model_reload_lock` 为目录发布及选择建立顺序；`SessionActor::admit_session_model_selection` 和 `admit_session_effort_patch` 持有 Step gate，在闭合 Step 应用并写入 Timeline。主动工具执行期间同步等待 finish 会自锁。

开始时已有未提交的 effort 暴露、仓库发现等工作。本 change 只叠加目标行为，保留原改动。

## Goals / Non-Goals

实现 proposal 的两项能力，并对相关实现链路做有证据的工程 review。工具不接受任意 session ID，不改变全局默认，不绕过 Workflow frozen route 或 child capability ceiling。普通 resume_from 派生新 child，与原身份 checkpoint restore 分开；后者不开放覆盖。专项审视不等同于全仓库正确性证明，无关债务只登记。

## Decisions

### Resume selection belongs to the child runner

TaskTool 传递显式参数，不复制 provider 校验。child runner 先完成现有 source 授权、canonical lifecycle/seed/result 检查，再选择最终配置。model 未给时固定来源模型及 transport；显式 model 时按当前 task-selectable catalog 解析新路由，解除旧 transport 对这次有意切换的限制。effort 未给时保留来源实际值（含 None），显式时用该值；按最终模型 offered efforts 校验。Agent 默认不能悄悄覆盖省略字段。无效选择不产生 provider 请求，不改来源历史，也不 fallback 为 fresh task。

这种逐字段覆盖让 resume 的省略含义保持稳定。来源实际选择取已校验 child Timeline 的最后 durable model selection，而不是父 Spawn 的出生选择；复用 `latest_model_selection`，含运行中用户/工具切换及最终 None。历史没有后续记录时沿用已有合法基线，不把读取或校验失败解释成缺失。增加来源先切换再结束后省略参数恢复的生产链回归。改用新模型默认会使未提供 effort 的调用隐式改变两项选择，因此不采用。来源原有路由不可用时可通过显式 model 选择新路由；缺少 override 仍按原规则失败。父 Spawn 和新 child Seed/route baseline 沿用现有持久化所有者，记录最终选择，不另建配置历史。

### The model tool is an adapter to the user control plane

新增 Grow namespace 原生 `set_session_model`，输入 `{ model?: string, effort?: string }`，拒绝两项均缺失、非 canonical effort 和未知字段；JSON null 表示未指定，`"none"` 为显式值。model-only 采用用户模型选择的默认 effort 语义；effort-only 使用 actor 当前最新 desired model，不能用调用时旧模型覆盖用户待应用选择。

工具绑定当前运行 session，身份来自宿主资源而不是模型参数。它通过既有工具 preflight/Hook/permission/permit 后，将请求交给 Shell 模型控制所有者。Shell 与 ACP 共享选择准备逻辑（usable/selectable catalog、allowed_models、offered effort、Workflow frozen route、origin credentials），并使用相同 catalog publication 顺序及 actor admission；新工具不维护平行模型状态或直接改 sampler。

工具只等待 actor 确认接纳到 pending Sampling slot，然后返回结构化 `queued`（不是 applied）。应用、supersession 和失败继续由既有 Step control、ModelChanged 与 Timeline 事实表达。不能仅凭 channel.send 成功报告已接纳，也不能等待当前 Step 结束才返回工具结果。处理完成/应用响应的既有 owner 继续负责其生命周期，不遗留无人持有的任务。新 native 工具采用现有 session-control 的无文件/进程 RWX 投影；工具 eligibility、精确 permit 和模型选择限制仍独立生效。

工具适配涉及 tools schema/registry/IO 和 Shell binding，是一项完整可验收行为；由同一 coder 维护接口一致性。actor admission 使用的已准备 `SessionSamplingSelection` 归 session command 契约所有，handler 只生成并发送，session 不从上层 handler 导入自己的命令类型。可在现有已绑定资源/信箱模式内选最小承载机制。若无法复用 catalog 顺序或 actor 接纳确认，必须先反馈证据修订设计，不用 sleep、fire-and-forget success 或额外状态库补洞。

### Review and ownership

resume 行为由 child-runner coder 负责，工具行为由模型控制 coder 负责；不并发写同一文件。架构 owner 维护 OpenSpec、用户说明与验收记录。独立 reviewer 使用当前 software-engineering/Rust 原则核对实际 diff 与调用方，关注 shared state、错误传播、重复规则、无依据抽象及真实入口验证。与本次不变量相关的发现逐项修复；无关债务写 backlog，禁止顺带扩建框架。

## Review Plan

Tier 3：新增模型可调用的控制入口，必须证明 target 绑定和现有 gate 生效。resume 从 model-facing JSON 到实际 child provider 覆盖四组合、explicit none、非法/unsupported、source 不可用但显式 override、source immutable 和 checkpoint restore 不变。新工具从真实调用到下一 provider 请求证明无自锁、闭合工具往返、model/effort 生效、effort-only 与用户 pending model 组合、非法选择无副作用、Workflow 和 capability/permission 边界、失败不冒充 applied。复用现有 gate、控制持久化和故障注入用例，只补新增入口缺口。

交付节点运行相关 tools/shell 单元与公共入口集成、默认 CLI check/build、格式校验、全部 OpenSpec strict 和归档校验。真实集成使用新建 mock provider 和本次 grow binary，不调用外部模型。独立 reviewer 由 magpie/gpt-6-luna 执行；架构 owner 验收整体链路。实现由 magpie/gpt-6.1-sol 执行。

## Risks / Trade-offs

- 工具同步等待应用会自锁 → 只回 actor 接纳确认，闭合 Step 后沿已有路径应用。
- 复制模型验证会使新入口绕过 allowed_models/Workflow → 将共同规则收回既有模型控制 owner，测试多入口。
- effort-only 捕获旧模型会撤销用户新选择 → 复用 actor 的 effort patch 组合。
- 解除 resume transport pin 可能扩散到 checkpoint restore → 显式区分派生 resume 与原身份恢复；后者沿用 frozen frontier。
- 目录发布与控制排序可能交错 → 新入口必须复用现有 publication 事务，不在未经同步的独立快照上拼 route。
