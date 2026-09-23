# Verification

## Implementation status — 2026-09-22

用户后续已授权专项实施。英文 primary/subagent audience、两个内置 role、task/task output 指引及相关装配测试已落入源码，开发者说明已同步。限定回归共 **240 项通过**。本 change 未归档，E01–E15 真实模型轨迹与 baseline/candidate 配对实验尚未执行；不报告提速比例，也不把静态文案审阅算作模型行为验证。

### Coordination and write scope

实施前与同工作树的两个任务确认所有权：

- 「梳理跨session交互并优化TUI」只修改 `clarify-agent-communication-context-and-rendering` 的设计文件，尚未改变生产协议。本次按当前 ask/send 语义实施，没有预写其规划中的新消息机制。
- 「排查 subagent 无法恢复」修改 shell subagent 生命周期、对应测试和开发指南，与本次八个生产/测试/说明文件没有重叠；其归档和主规范更新没有计入本次成果。
- 四个 Markdown 文件交给有界 subagent 修改；主线程拥有工具说明、装配测试和本 change 文档。子任务交付后重新检查正文、保护区和实际渲染，并进行了只读语义复核。

本次修改的八个既有文件为四份 audience/role Markdown、`crates/common/tool-types/src/task.rs`、`crates/codegen/agent/src/prompt/{context,template}.rs` 和 `crates/codegen/agent/PROMPT_ARCHITECTURE.md`。最后一份仅追加两个协作分层段落；实施前已有权限说明完整保留。没有修改 `builder.rs`、共享 standard/mandatory-core、任务 schema、通信/权限/恢复运行时、`docs/architecture/local-coordination.md` 或主规范。

### Baseline and preserved regions

重新核对时，task.rs、context.rs、template.rs 的 SHA-256 与下方设计基线完全相同。四份 Markdown 的实施前内容也已重建并逐一验证与下方基线摘要相同：保留当前文件非目标区段，以 HEAD 中原 audience/role 正文还原基线。该方式没有以 HEAD 覆盖用户已有权限变更。

| 生产文件 | 实施前 bytes | 实施后 bytes | 差值 |
| --- | ---: | ---: | ---: |
| `prompts/audience/primary.md` | 1,145 | 5,538 | +4,393 |
| `prompts/audience/subagent.md` | 1,624 | 4,311 | +2,687 |
| `prompts/agents/general-purpose.md` | 734 | 578 | −156 |
| `prompts/agents/explore.md` | 719 | 932 | +213 |

上述路径相对 `crates/codegen/agent/`。保护区检查通过：primary 的工具速记不变；subagent 的 capability authority、权限说明和 ask_parent 尾部不变；两个角色 YAML frontmatter 不变。四个目标区段逐字匹配 `prompt-drafts.md` 第 1–4 个英文块，全部 10 个英文示例块无中文。task.rs 除两个 description 函数和测试外逐字相同；context/template 的非测试代码逐字相同。

### Executed implementation checks

Rust 回归使用相同的构建环境，限定包和测试过滤器：

```sh
export CARGO_TARGET_DIR=/tmp/grow-prompts-check.zxgNtj
export CARGO_PROFILE_TEST_DEBUG=0
export CARGO_INCREMENTAL=0
export CARGO_BUILD_JOBS=2
export RUST_MIN_STACK=16777216

cargo test --locked -p agent --lib prompt:: -- --nocapture
cargo test --locked -p agent --lib builder::tests::build_task_description
cargo test --locked -p agent --lib builder::tests::child_task_description_is_concise
cargo test --locked -p tool-types --features prompt-render --lib task::tests:: -- --nocapture
```

| 检查 | 结果与证据 |
| --- | --- |
| agent `prompt::` | exit 0，203 passed；[测试输出](validation/agent-tests.txt) |
| builder task description | exit 0，9 passed；[测试输出](validation/builder-tests.txt) |
| child task 简洁约束 | exit 0，1 passed；[测试输出](validation/child-task-tests.txt)，原 700-byte guard 保留 |
| tool-types `task::tests::` | exit 0，27 passed；[测试输出](validation/tool-types-tests.txt)，含后台默认、schema、动态命名、wait-all、参数重命名 |
| scoped `rustfmt --edition 2024 --check` | exit 0，仅检查 task.rs、context.rs、template.rs |
| scoped `git diff --check` | exit 0，未对其他任务变更进行格式化 |
| `openspec validate --all --strict --no-interactive --json` | exit 0，21/21：7 changes、14 specs，0 failures；与设计阶段数量差异来自并行任务归档 |
| 独立只读实施审阅 | 无阻断问题；正文、delta、工具语义与装配边界一致 |

失败与处理均保留说明：

1. 首次 agent 编译因新增测试复用非 Copy 的 `PromptComposition` 产生 E0382；仅修正测试中的 clone，重跑后编译通过。
2. 随后 200 项通过、3 项长度检查失败。完整 child 指引为 8,038 bytes，read-only 组合为 7,563 bytes；按本次纯文本增量反推，实施前分别为 5,351 和 4,876 bytes，旧 5,000/4,500 上限在当前工作树基线上已不适用。
3. 保留全部三个检查及 read-only 小于完整组合的关系，显式将上限定为 8,192 / 7,680 bytes，修正错误的 chars 单位；重跑 203 项全部通过。这是接受已审阅文案输入成本的预算调整，不是压缩收益或模型效率验证。

新增装配回归通过 `discovery::by_name` 解析实际内置 Markdown，再覆盖 Primary/Subagent × general-purpose/explore × Extend/Full 的 8 个组合。无 task 工具的 renderer 仍能得到完整 audience；角色切换与 Full 不能移除固定 head、子权限段或混入角色正文。这里测试 PromptContext 组合边界，不表示 `subagentOnly` 角色可以进入 primary picker。既有工具组合、无可用工具、模板变量保护及 builder 回归继续通过。

### Rendered input cost

下表单位为 UTF-8 bytes。candidate 的 head/role 来自真实渲染测试；baseline 由已验证的旧文件与纯文本差值推导，未把推导值伪称为独立 baseline 模型运行。空工具集、memory=false；只统计固定 head 和角色层，不含项目规则、工具 schema、历史或任务输入。

| 层 | Baseline | Candidate | 差值 |
| --- | ---: | ---: | ---: |
| Primary stable head | 3,757 | 8,150 | +4,393 |
| Subagent stable head | 4,236 | 6,923 | +2,687 |
| General-purpose role，Extend | 856 | 700 | −156 |
| General-purpose role，Full | 496 | 340 | −156 |
| Explore role，Extend | 777 | 990 | +213 |
| Explore role，Full | 417 | 630 | +213 |
| Primary task helper，两条测试 Agent、literal names | 1,453 | 2,103 | +650 |
| Task output helper，CLI 默认命名 | 587 | 878 | +291 |

例如 subagent general-purpose 的 Extend head+role 从 5,092 增至 7,623 bytes；explore 从 5,013 增至 7,913 bytes。新装配测试对 head+role 保留 16 KiB 上限。工具描述测量采用固定测试 roster，不代表每个用户自定义 Agent/模型目录下都只有该大小。没有指定模型 tokenizer 的精确 token 测量；不由 bytes 推算 token、费用或提速。

### Delta scenario evidence and remaining model evaluation

以下覆盖全部 7 个 requirement、20 个 delta scenario。状态“指引已实施”只表示模型可见文本、装配/工具语义和保留边界经过验证；每项对应的真实模型行为均仍待运行。

| Requirement / scenario | 实施与检查证据 | 后续行为矩阵 |
| --- | --- | --- |
| Audience ownership — Primary retains overall responsibility | primary 首段与第 1/8 节；职责锚点、真实装配 | E15 |
| Audience ownership — Child retains bounded responsibility | child 首段与第 1/6/7 节；权限保护区、真实装配 | E15 |
| Audience ownership — Role composition varies | discovery 的 8 组合、既有 head/role 分离测试 | E15 |
| Delegation — Consumer needs only a confirmed interface | primary 第 2/4 节；只读语义审阅 | E02 |
| Delegation — Task is too small to benefit from delegation | primary 第 2/7 节；child task 简洁限制保留 | E01 |
| Delegation — Task-specific context is required | task 工具新说明；参数替换及 builder 回归 | E02 |
| Readiness — Design input is unresolved | primary 第 3/5 节、child 第 2 节；有界假设审阅 | E03 |
| Readiness — Candidate result has returned | primary 第 8 节、child 第 6/7 节；验收锚点 | E12 |
| Ready work — One result arrives before an unrelated task | primary 第 4/5 节；task output 新指导 | E04 |
| Ready work — Useful independent work remains | primary 第 5 节；后台指导与默认回归 | E05 |
| Ready work — No useful independent work remains | primary 第 5 节；无忙轮询指导 | E06 |
| Ready work — Multiple task wait is selected | task output snapshot、wait-all 和重命名回归 | E04/E06 |
| Ownership — Results accumulate faster than review | primary 第 7 节；独立审阅 | E13 |
| Ownership — Parent would overlap an active writer | primary 第 6 节、child 第 4 节；安全交接锚点 | E07/E09 |
| Ownership — Shared premise changes | primary 第 6/8 节、child 第 4 节；局部停止锚点 | E08/E14 |
| Inquiry — Clarification cannot perform required parent action | child 第 5 节；部分结果/阻塞锚点与现有能力段 | E10 |
| Inquiry — Intermediate evidence is queried | child 第 5 节；保留工具说明，不新增主动通道或工具验证承诺 | E10 |
| Evidence — Local tests pass but integration remains | primary 第 8 节、child 第 6/7 节；职责审阅 | E12 |
| Evidence — Exploration reaches sufficient evidence | 实际 explore Markdown、discovery 回归 | E11 |
| Evidence — Returned assumptions become stale | primary 第 6/8 节、child 第 4/7 节；前提审阅 | E14 |

E01–E15 未运行，未生成模型工具轨迹；固定模型/effort/工具/权限/并发/初态的配对实验也未运行。现有静态审阅、Rust 测试以及本次 Codex 自身协作过程均不能替代候选 Grow 系统提示词下的模型实验。tasks 4.3、4.4、5.1、5.2 保持未完成；完成实验并逐场景接受结果后再做最终归档验收。

文案通过正常构建进入后续新会话；本次没有发布/安装 CLI，也没有热替换已有会话稳定 head。临时 target 约 1.6 GiB，验证后已清理。`cargo clean --target-dir` 因预先创建的临时目录没有 `CACHEDIR.TAG` 而拒绝清理（exit 101）；随后确认规范化路径、目录内容和本次测试产物，只删除本次创建的私有 target。未清理其他任务的共享 target。

### E01–E15 可行性审计（2026-09-23）

本次只读核对没有启动模型推理，也没有读取或输出任何凭据。E01–E15 仍不能在当前环境中作为真实模型验收完成，原因和边界如下：

- `crates/codegen/test-support/src/mock_server.rs` 的 `MockInferenceServer` 只提供 echo/fixed 文本和 `ScriptedResponse` 回放；`crates/codegen/test-support/src/inference_override.rs` 的匹配条件只有 endpoint 与 foreground/auxiliary 类型。它可以重放预先写好的 tool-call SSE，但不会根据候选 prompt 作模型决策，不能替代真实模型或 baseline/candidate 行为比较。
- `crates/codegen/test-support/src/headless.rs` 能把 binary 接到 mock server，但仓库没有 E01–E15 的场景交错驱动器或完整工具轨迹导出器；因此现有协议/执行 fixture 无法证明等待、失效前提、交接和验收选择。
- `grow --version` 只确认可执行文件为 `grow 2.1.11 (3024dad)`；`/Users/lordcasser/.local/bin/grow` 的修改时间为 2026-09-17，不能作为包含当前未提交 prompt 改动的 candidate。遵守低磁盘约束，本次没有 Cargo 构建。
- 本机 LM Studio 服务状态检查显示 loopback `127.0.0.1:1234` 正在运行，但没有加载模型；磁盘上只有 `minicpm5-2b` 与 embedding 模型。该本地 provider 可能作为后续无付费实验入口，但尚未验证工具调用稳定性、场景能力或耗时，不能将其存在记为实验通过。

后续真正执行前至少需要：当前 candidate binary；隔离 `TestSandbox` 与 keyless loopback provider 配置；一个能制造每个 E 场景依赖/交错并保存完整工具轨迹的 driver；以及固定模型、effort、工具、权限、并发上限、初始代码和外部输入。应先对每个 E 做一条 pilot，确认模型确实会产生所需的 task/task-output/send/interrupt 等调用，再运行配对样本。

范围与统计口径按 [validation-plan.md](validation-plan.md) 保持不变：15 个场景各一条只能证明场景被执行；计划建议的每场景 3 个 baseline/candidate 配对，即 `15 × 3 × 2 = 90` 次模型调用，仅是初始筛查，不能声称统计显著。若要做最低可信的统计比较，建议至少每场景 10 对，即 300 次调用，或先按预期正确率差异做 power analysis。当前没有固定模型、tokenizer、输出上限或价格，因此不能诚实给出费用和耗时；应以实际 trace 的 input/output tokens 与墙钟时间记录，不能用现有 headless 的 60 秒 scripted-test timeout 推算模型成本。

本审计只补充可复核的未执行限制，不勾选 tasks 4.3/4.4，也不执行归档。

## Historical design-phase status

以下保存 2026-09-22 初次方案交付时的历史记录；其中“本轮”和“当前”只指当时的设计阶段，最新实施结果见上文。

- 本轮允许修改范围仅为 `openspec/changes/improve-dependency-driven-agent-prompts/`。
- 已读根 AGENTS、OpenSpec 索引/config、开发指南相关流程、协作/权限/Behavior 契约及相关进行中 change。
- 当前主规范仍是已归档行为权威；本目录 delta 表达待实施指引，未合入 `openspec/specs/`。
- 未修改生产 prompt、Rust、开发者说明或配置；未运行 Rust 测试、模型行为评估或归档。
- HEAD 为 `3024dad1ef75a29b1e94a3b5e47083255ad5676e`。工作树已有广泛未提交修改；尤其 subagent audience、builder、PROMPT_ARCHITECTURE 与权限实现已有其他任务变更，均保留原样。

### Design baseline evidence

以下为开始写文档前读取的关键文件 SHA-256。结束时重新核对同一集合，结果见下方执行记录。

| Path | SHA-256 |
| --- | --- |
| `crates/codegen/agent/prompts/audience/primary.md` | `4bc07ccdc5c015131cb69fe69d9a78a26d48e3012a94a6986d411642d81edce1` |
| `crates/codegen/agent/prompts/audience/subagent.md` | `6617c5d468fdf8ee8fe00c645c5376ff0e9700713b491501f2fa3c32e07b27b0` |
| `crates/codegen/agent/prompts/agents/general-purpose.md` | `30925384c76b66138af6dbc30237eca9ed11ac0ae02c53b7b4d43946d475be87` |
| `crates/codegen/agent/prompts/agents/explore.md` | `b2592c27dbd0c89ae4a8c0538515ba649638674277dc3e4e7c9e02f96b15d3f9` |
| `crates/codegen/agent/prompts/foundation/standard.md` | `9036fb418a6f1bd571677e9cc4045f9a744be31d3bb010abb1150ca0360c514e` |
| `crates/codegen/agent/prompts/foundation/mandatory-core.md` | `5accd88f15cb844b7461a3f94090277d80b7aa065fcd43386cb18842b11add97` |
| `crates/common/tool-types/src/task.rs` | `557a9c3144304afb7fc06c7fcfef4bb7cb7efe9f8fc439f9a2b6e007d1020f90` |
| `crates/codegen/agent/src/prompt/context.rs` | `2848b73c8ce06ed68606eb045d5ac49c687a81912669f5be80974c387e3ff174` |
| `crates/codegen/agent/src/prompt/template.rs` | `e536a2e84459a14674ee440ea5865efe10227d3520fcf8e7c917ce6d490519f9` |

直接源码核对的核心入口：

- `PromptContext::render_with_renderer` / `render_role_with_renderer`：固定 audience 与可变 role 分层。
- `AgentBuilder` task description 分支：primary full description 与 child concise description 不同。
- `TaskToolInput::run_in_background`：实际默认 `true`；以 serde/schema 和测试为证，不采用旧注释。
- `build_task_output_description` / `wait_all_event_driven`：多 ID 正超时为 wait-all。
- `AskParentTool` / `AskSubagentTool` / `SendSubagentMessageTool`：询问、前台指导和持久接收回执边界。
- 当前 audience / role 全文、模板职责断言、Full/Extend 分离测试与角色发现入口。

### Design-phase checks

| 检查 | 实际结果 |
| --- | --- |
| `openspec --version` | `1.11.0` |
| `openspec validate improve-dependency-driven-agent-prompts --strict --no-interactive --json` | exit 0，1/1 change 通过，issues 为空 |
| `openspec validate --all --strict --no-interactive --json` | exit 0，22/22 通过：8 changes、14 specs，0 failures |
| `openspec status --change improve-dependency-driven-agent-prompts --json` | proposal/specs/design/tasks 四类 artifact 均齐备；该结果不代表实施完成 |
| `git diff --check -- openspec/changes/improve-dependency-driven-agent-prompts` | exit 0；因文件尚未跟踪，另外执行正文检查 |
| 文档正文检查（Python，检查全部 8 个 Markdown） | 相对链接存在、尾随空格/末尾换行/围栏检查通过 |
| 候选语言检查 | 10 个 `text` 围栏均无中文，包括两份 audience、两份 role、工具说明及委派示例 |
| requirement 到行为矩阵覆盖检查 | 7 个 delta requirement 都出现在 `validation-plan.md` 的对应场景中；E01–E15 是待执行实验，不是已通过实验 |
| 生产文件摘要复核 | 上表 9 个文件 SHA-256 全部与开始编写前一致 |
| 独立只读语义审阅 | 修正 primary 草案中 `same write boundaries` 的歧义，明确尊重各自所有权而非主从拥有相同写入范围；其余未发现阻断问题，未承诺不存在的通信或等待能力 |

静态体量记录：primary 候选 `<audience>` 为 5,394 UTF-8 bytes / 732 个按空白分隔的词，subagent 为 3,133 bytes / 434 词。这不是 tokenizer token 数，也不是完整渲染 System head；完整体量与推理成本留待实施评估。

根据以上文档与源码核对，只勾选 tasks 1.1–1.3。第 2–5 组全部保持未完成。没有运行 apply 或 archive。

### Not executed at design phase

- 第 2–5 组实施、Rust 回归、E01–E15 模型场景和配对性能实验均未执行。
- 未证明候选文案降低耗时或 token；不存在可报告的提速比例。
- 未执行 `openspec archive`，不能把 artifact 齐备等同于实现完成。
- 本轮不检查 unrelated change 的实现正确性；全量规范校验若暴露其他目录问题，只报告归属，不顺带修改。
