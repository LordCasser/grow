# 验证计划

本文件保留原定验证矩阵；实际执行、2026-09-24 的成本/价值复核和收窄后的归档判据以 [verification.md](verification.md) 与 [模型 pilot](validation/model-pilot.md) 为准。格式校验、渲染回归和真实模型行为实验提供不同层次的证据，不能互相替代。

## 1. 文档与范围检查

- proposal capability 与 delta 路径一致；全部新增要求有 SHALL/MUST 和 WHEN/THEN。
- 英文文案均有唯一目标文件与替换范围；实施仅修改授权区段，工具 schema 和主规范不提前变更。
- tasks 区分设计、实施、确定性验证和真实模型评估；不将已有权限、通信或取消修复算成本次结果。
- 交叉链接可解析，引用路径和符号存在；候选未添加 Jev、运行时状态枚举或主动 child→parent 指令通道。
- 保存关键生产输入的 SHA-256，结束时重新比对；若有并行任务修改，明确报告，不恢复他人的变更。

当前阶段执行：

```sh
openspec validate improve-dependency-driven-agent-prompts --strict --no-interactive
openspec validate --all --strict --no-interactive
openspec status --change improve-dependency-driven-agent-prompts
git diff --check -- openspec/changes/improve-dependency-driven-agent-prompts
```

新建未跟踪文件还需单独检查尾随空格、缺少换行和未闭合围栏；`git diff --check` 本身不覆盖全部 untracked 正文。

## 2. 实施后的确定性回归

以下命令在生产 prompt 与相应测试实施后运行；实施环境、实际结果和磁盘控制见 verification：

```sh
cargo test --locked -p agent --lib prompt::
cargo test --locked -p agent --lib builder::tests::build_task_description
cargo test --locked -p agent --lib builder::tests::child_task_description_is_concise
cargo test --locked -p tool-types --features prompt-render --lib task::tests::
```

这些命令使用已核对的 Cargo 包名 `agent`、`tool-types`。未来改动若引入其他受影响测试，再按实际失败或证据扩大范围，不默认跑整个 workspace。

| 验证对象 | 必须覆盖 | 判定边界 |
| --- | --- | --- |
| primary audience 渲染 | 全局责任、依赖、三种就绪、独立工作、验收、前提失效、并发收敛 | 指引确实被包含，不证明模型总能正确调度 |
| subagent audience 渲染 | 局部自治、范围、假设、询问边界、返回证据 | 保留现有 capability authority，不新增父 Goal 权限 |
| Extend / Full | 两种 composition 都保留固定 audience；role/standard 的差异保持 | 不通过把规则复制到 standard 来掩盖 Full 漏注入 |
| 正常内置 role 发现 | 实际加载 Markdown 的 general-purpose/explore | 不仅用 common legacy 常量构造 child fixture |
| task 工具说明 | 动态工具/参数名、Agent roster、自定义定义覆盖、委派信息、后台默认 | 不硬编码 namespace；不增加 schema 字段 |
| child task 描述 | 仍受原有简洁与递归限制 | 保持小于现有 700 字节 guard；不塞入父完整 roster |
| task output 描述 | 多 ID 正超时 wait-all；零/省略超时为 snapshot；wait cap | 不宣称 wait-any、即时通知或无限等待 |
| 权限不回退 | capability authority 区段及 relevant frontmatter 前后保持 | 文案不撤销既有 required-review 或 hard eligibility |
| 文本体量 | 分别记录 head、role、工具说明的字节数；有真实 tokenizer 时记录 token 差值 | 16 KiB guard 只是上限，不能当作应填满的预算 |

既有关键词测试允许随正文改写更新锚点，但应保留其保护的职责。新增测试优先验证实际装配、不同 audience/工具组合和必要约束，避免逐句镜像整个草案。

若仅修改文案和装配断言，不需要重新实现 Sideband、wait-all 或 permission Gate 测试；相关运行时应通过既有测试或源码证据确认保持。如果实现意外触及这些路径，应重新审查 scope，再决定额外回归。

## 3. 行为场景

下面场景是原定的诊断矩阵，不增加正式产品状态机或专用评测框架。可复用现有测试/会话机制；fixture 应使用可恢复的隔离工作区和确定的输入，不操作外部服务。它们不再全部作为本次“指引文本被正确交付”契约的归档门槛；只有实际制造的条件才能记为模型行为证据。

| ID | 设置与触发 | 预期可观察行为 | 失败信号 | 对应 delta |
| --- | --- | --- | --- | --- |
| E01 | 一个局部、低成本且独立的小修正 | 直接形成最小闭环，验证与风险相称 | 为占用并发槽位拆出多个需要频繁同步的任务 | Delegation guidance names sufficient inputs and acceptance |
| E02 | 共享接口已确认，编码器与客户端分别有明确写入范围 | 两项工作可并行，parent 准备集成或处理独立问题 | 等编码器全部结束后才启动仅依赖接口的客户端 | Delegation guidance names sufficient inputs and acceptance |
| E03 | 接口决策未定，但已有迁移/调用机制可调查 | 有界探索继续，正式实现等待决定 | 把假设接口直接写入共享实现 | Coordination guidance distinguishes exploration execution and acceptance |
| E04 | A 先返回，B 仍运行，C 只需 A | 在自然检查点验收 A 后推进 C | 直接对 A/B 长时间 wait-all，C 无关地停住 | Primary guidance advances independently ready work |
| E05 | child 执行一项调查，parent 有不同范围的验收准备 | parent 做独立准备，避免重复同一调查 | parent 重做 child 的任务或接管其文件 | Primary guidance advances independently ready work |
| E06 | 全部剩余工作确实等待 child | 使用有界等待或已有让出机制 | 忙轮询、不停重查同一结果、制造无关工作 | Primary guidance advances independently ready work |
| E07 | 两项任务拟修改同一共享范围 | 明确所有者并安排交接，或选择已有隔离方式 | 同时写入同一范围；声称 worktree 消除了语义依赖 | Coordination guidance bounds concurrency and write ownership |
| E08 | 已确认接口变化，A 依赖它，B 不依赖 | 通知并停止 A 受影响部分、重查已返回成果；B 继续 | 全部任务无差别取消，或继续沿失效前提写入 | Coordination guidance bounds concurrency and write ownership |
| E09 | 发送中断指导已 durable received，但 child 的不可中断工具还未结束 | parent 等安全交接证据，再接管相关范围 | 收到回执即假定 child 已停止并开始重叠写入 | Coordination guidance bounds concurrency and write ownership |
| E10 | child 需要 parent 实际改共享接口，ask_parent 只能澄清 | 返回部分成果、证据与所需决定，由父前台后续处理 | 双方循环等待；把 Sideband 回答当作执行或权限授予 | Child guidance respects inquiry and delivery boundaries |
| E11 | explore 已找到足够证据，但全仓仍有其他可搜索内容 | 停止，报告路径、覆盖与剩余未知 | 无限扩大调研；将局部未找到断言为全仓不存在 | Delegated results carry evidence and bounded conclusions |
| E12 | child 局部测试通过，跨模块集成尚未做 | child 清楚返回限制；parent 验收并做必要集成检查 | parent 将 runtime completed 直接作为全局完成 | Delegated results carry evidence and bounded conclusions |
| E13 | 多个结果待验收，另有易派发任务 | 优先消化关键结果，控制新增并发 | 持续派发导致关键成果长期积压 | Coordination guidance bounds concurrency and write ownership |
| E14 | 已验收过的某个结论依赖输入发生变化 | 只重新检查受影响结论及产物 | 继续引用旧成功报告，或无依据全量重跑所有检查 | Delegated results carry evidence and bounded conclusions |
| E15 | Full/concise 或不同 built-in role；父 Behavior 限制动作 | 仍遵守 audience 责任和现有 Behavior/权限 | 省略关键证据、越过 Plan 限制或接管父 Goal | Coordination guidance preserves audience ownership |

场景允许不同正确执行顺序。判定依据是依赖和证据，而不是固定 tool-call 序列、强制 Agent 数量或固定措辞。若当前可用工具无法制造某个确定性交错，应记录限制，不能把未执行写成通过。

## 4. 对照与指标

对同一 fixture 比较当前 baseline 和候选 prompt，固定模型、effort、工具、权限、并发上限、初始代码与外部输入。原计划建议每场景至少 3 组配对以暴露波动；15 场景合计至少 90 次模型运行，仍不足以做统计显著性结论。2026-09-24 决定只做三种不同任务形态各一组受控配对，作为明显回退的探针，不计算总体正确率或提速率；记录在 [模型 pilot](validation/model-pilot.md)。

对每次运行记录：

- 是否正确完成、是否遗漏必需验证、是否越界或使用失效前提。
- 从开始到经验证交付的实际耗时；未完成运行不作为成功耗时样本。
- 总 token 与可取得的主/子任务用量；不可取得时标记 unknown，不推算费用。
- 完成通知到首次有效验收/采用的延迟。
- 有就绪且有价值工作时的无效等待，基于可观察工具和消息时间戳标注。
- 重复调查、同范围重叠修改、失效前提返工和无必要的递归委派。
- prompt 输入增量及是否抵消减少的协调回合。

接纳条件：正确性与关键边界场景不得因追求速度退化；E04/E05/E13 等目标场景能观察到预期工作选择；耗时与 token 的变化如实报告，不以 Agent 数量增加代替改进。若收益不稳定或成本上升，应缩短/修订文案并复核受影响场景，不先宣称整体效率提升。

结果记录格式可为 change 内 Markdown 表，列出 baseline/candidate、fixture、模型设置、运行证据路径、结果、耗时、用量与失败归因。无须新增产品 telemetry 或评测服务。

## 5. 归档前检查

只有 tasks 中实现、开发者说明、装配回归、有界 pilot 和未执行限制记录完成后，才运行：

```sh
openspec validate --all --strict --no-interactive
openspec archive improve-dependency-driven-agent-prompts --yes
openspec validate --all --strict --no-interactive
openspec validate --archived --no-interactive
```

OpenSpec 显示 artifact complete 只代表文件齐备，不代表上述实现或实验完成。归档时只接纳 delta 中规定的指引交付契约；不将未执行的行为矩阵转换为“模型已遵守”的结论。
