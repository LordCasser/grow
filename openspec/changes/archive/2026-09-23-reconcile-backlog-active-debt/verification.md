## Audit basis

逐段核对 `openspec/backlog.md` 与当前代码、主规范及归档验证。原文件 351 行，混有旧调查过程、已解决 bug、仅为未来产品选择的提示及真实未完成边界；清理不通过删除失败场景来宣称代码已修复。

| 判定 | 代表性证据 | 文档处理 |
| --- | --- | --- |
| 已完成且归档 | `project_portable_history` 保留配对工具交换；Workflow restore 对有效候选计数；`local-coordination` 已有父子消息 durable receipt、重试/恢复契约；Slash/Pager/配置多项旧修复见各归档 change | 删除旧时态待办，保留行为权威在主规范 |
| 代码入口已不存在 | `diagnostics::id::agent_id` 与 `snapshot_session_log` 已不在当前 diagnostics crate；`snapshot_log` 仅有测试消费者；`apply_remote_settings_side_effects` 和其可选图片缓存入口也不在当前代码中 | 删除对应生产风险描述 |
| 只是猜测或未来选择 | Status line 搁置、工作树生命周期观察、技能 `$N` 语法统一、子 Agent typed eligibility reason/native policy 继承、尚未提供的子视图用量入口和图片草稿恢复 | 删除无触发场景的开放事项 |
| 混合记录 | Debug writer、图片资产重试、TTL 扫描、rewind 旧读结果、采样日志凭据、CLI Trace | 去掉已修复前半段，只保留资源/归属/语义的剩余缺口；Trace 修复后不另留无故障触发的“全 CLI 审计”泛化任务 |
| 独立真实债务 | Memory flush 丢工具结果、`search_replace` 跨进程提交保护、Workflow checkpoint/Forgotten、child FD 继承、Folder Trust path race、ACP bridge 取消通知 | 保留具体触发、影响和验收边界，不能为使列表变短而删除 |

Pager 的宽泛“职责混合/坐标模型/错误文案可能漂移”条目缺少复现或可判定的违反契约场景，原始代码证据仍可在 `2026-09-23-inventory-all-crate-features` 的 draft-deltas 查到。backlog 仅保留共享进程状态、未界定 worker 队列、运行时 `expect`、协调行 identity、授权细节上限、列表缓存 panic 与 session picker cwd 等有明确注入入口的风险；未将被移除的推测称为已修复。

MCP 2026-07-28 已[正式发布](https://blog.modelcontextprotocol.io/posts/2026-07-28/)；[官方 Rust SDK roadmap](https://github.com/modelcontextprotocol/rust-sdk/blob/main/ROADMAP.md) 与 [TypeScript SDK 迁移说明](https://ts.sdk.modelcontextprotocol.io/v2/migration/support-2026-07-28) 均显示 MRTR 支持。旧“等待非 draft 规范/SDK”的前提已过时；但尚无 Grow 的具体 Form/URL 使用场景及客户端/服务端互操作需求。MCP Elicitation 属于未获产品需求的新增能力，不作为待修 bug 保留；需要时应从 2026-07-28 MRTR 契约重新立项，不能复用旧服务端主动挂起设计。

Markdown fuzz README 漂移由独立 docs-only change 修复。现有 fuzz target 明确是 crash-oriented；没有先定义稳定语义不变量或失败 corpus 时，增加差分/属性 oracle 只是测试方案偏好，不继续占据 backlog。`search_replace` 非 NotFound 读取错误、bracketed paste 事件边界、Slash MRU 写入预算、Workflow 损坏 sidecar 和 Skill 正文读取预算均由各自独立 change 处理，本 change 不混入行为实现。Workflow 修复已归档为 `2026-09-23-repair-corrupt-workflow-manifest`，损坏快照只在原始指纹仍匹配时替换，恢复 actor 等待持久 ACK；有效 manifest 的 revision CAS 单测继续通过。

Slash MRU 的临时文件冲突、加载/写入 1 MiB 限制与后台待写数量均已有归档回归。剩余的跨进程 last-writer-wins、退出 best-effort 和持续超限 dirty 属于低价值 recency 数据的既定保证；当前没有用户可见的具体失败要求值得引入跨进程合并或同步退出协议，因此移出 backlog。完整序列化向量仍先分配；若出现可复现的内存峰值再单独立项。

公告隐藏偏好的主规范只承诺单 AppView 内串行并合并待写快照、完整原子替换及 1 MiB I/O 接纳；未承诺跨 Grow 进程合并或退出 drain。旧条目以“若产品要求”假设一个更强的新保证，当前没有对应用户场景或失败证据；该低价值偏好不值得仅为假设引入跨进程合并协议，故移出 backlog。现有写入失败传播契约保持不变。

搜索索引 bootstrap 对损坏/暂缺 summary、Timeline 读取/索引失败的旧行保留、完成标记与 Recheck 恢复，已由 `2026-09-23-retain-session-search-index-on-incomplete-bootstrap` 归档，并通过 76 项定向测试。TTL cleanup 仍按进程内 `Once` 做启动时 best-effort，失败记录错误并保留候选，下次进程启动可再清理；主规范没有同进程重试或 durable completion 保证，旧条目把这项未来产品选择与已修复的搜索索引错误混写，故删除。

Recap 跨离开周期审计确认重试退避不会被迟到通知修改，但旧周期自动结果仍可在 Grow 与 ACP 两条无全序的流间迟到，把新周期标为已展示。独立 `2026-09-23-bind-auto-recap-to-away-period` change 已让自动请求及通知携带 Pager 生成的周期身份，在展示前丢弃旧周期结果；Shell 33/33 与 Pager 47/47 项 Recap 定向测试通过，故删除该条目。

Recap 的旧“动态能力更新”条目并无当前可达的热切换故障：Shell `initialize` 和 `grow/recap` 接纳读取同一运行时配置，ConfigReloader 不热更新 `session_recap`；Pager 只在初始化或重连时更新能力。文件中的开关变化在重连前不改变服务端 gate，重连时重新发布能力；`disabled:true` 仍可清理意外的手动请求进度。主规范未承诺热切换，故移除对新能力协议的假设性待办。未来若提出 live toggle 产品需求，应另立明确的配置/通知顺序契约。

工具展示的封闭枚举覆盖已由 `2026-09-23-complete-tool-call-presentation` 完成：LSP 与动态工具的生产开始事件保留具体身份，ContextRecall/动态结果关闭原工具行，两个转换 match 已无通配分支。Behavior projection 的 revision 广播与 Pager 旧快照过滤由 `2026-09-23-refresh-behavior-availability` 完成，Shell 两项与 Pager 一项定向测试通过。这两项从 backlog 删除，归档主规范保留契约。

Sideband 用量审计未发现重复累计，但发现明确的展示口径差异：Active Goal 通过 GoalTokenUsage/root ACK 累计 Sideband；会话 UsageLedger、`/usage` 与 headless totals 仅含主采样及已折叠子 Agent。backlog 将原先假设性的“若需要统一”改写为产品口径决策，并给出若要求完整模型消费时按每次 Sideband attempt 持久归属、覆盖重试且不重复累计的验收入口。

技能描述字段的宽松类型确有可复现影响：布尔/数字原先可能被字符串化为 slash 身份或模型选择文本，`allowed-tools` 混合列表可能被部分展示。独立 `2026-09-23-validate-skill-descriptive-metadata` change 已严格处理这些字段，tools 技能发现定向测试 43/43 通过，条目据此删除。

## Validation

- backlog 最终为 117 行、43 个粗体待办条目（原 351 行）；4 个本地 Markdown 链接均存在（独立脚本按文档路径解析）。外部 MCP 链接为官方发布/SDK 文档。
- `git diff --check` 通过；`openspec validate --all --strict --no-interactive` 在本 docs-only change 归档前为 18/18、归档后为 17/17；`openspec validate --archived --no-interactive` 在归档后为 383/383。
