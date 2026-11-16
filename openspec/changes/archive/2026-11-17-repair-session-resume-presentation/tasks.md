## 1. 当前 Behavior 恢复

- [x] 1.1 Shell load 从有效 actor 提供带 epoch/revision 的当前 Behavior 快照，冷恢复与 resident reconnect 复用同一投影；以无模式历史页、保存失败和 live selection 竞争的模块测试验证快照来源与持久边界（Tier 2）。
- [x] 1.2 Pager 将 load 与实时模式通知接入同一已确认状态 reducer，历史页保持只读；以最近页没有切换记录、快照晚到及旧 epoch/绑定回包测试验证不回退、不误结算本地 intent（依赖 1.1，Tier 2）。

## 2. Plan 调研权限与提示词

- [x] 2.1 工具授权 owner 移除基于 Plan phase 的 Bash/Edit/MCP/Task capability 禁用，保留普通权限与协议互斥；按相同参数比较 Normal/Plan 的 allow/ask/deny、子 Agent 上限与 Sandbox 结果，并保留 PlanControl 审批测试（Tier 3）。
- [x] 2.2 Plan 提示词 owner 同步 base/phase contexts，明确工具仅服务调研、禁止直接或委派绕过审批；用允许终端调查、诱导顺手实现、委派实施、MCP 旁路和修改冻结计划的代表性模型任务验证，记录静态检查与实际行为评测各自边界（Tier 3）。

## 3. 历史页与目录服务

- [x] 3.1 Shell history owner 提供同 cut 的 before/after 页和 source/entity/fragment envelope，生命周期不依赖 olderCursor；对不同方向、重叠、超大响应、Tool continuation、分支失效和固定预算与 eager projection 比对（Tier 2）。
- [x] 3.2 Shell history owner 在现有索引发布与读取路径提供可见 UserPrompt 的轻目录及按 ordinal 查询，保留重复文本发生身份；验证目录不读取全部正文、rank/source 一致、非法/旧 cut 拒绝（Tier 2）。
- [x] 3.3 Shell history owner 提供 target around 读取，返回目标及两端 continuation；对最早、最晚、大消息邻近目标和 transformed branch 验证直接 seek 的有界工作量（依赖 3.1、3.2，Tier 2）。

## 4. 主 view 连续浏览

- [x] 4.1 Pager 历史投影 owner 按 source/entity/fragment 将页面合并进活动主窗口，维护两端边界和 viewport/selection；跨页响应及 Tool 乱序/重读测试与完整投影一致，替换原“只进 viewer”断言（依赖 3.1，Tier 2）。
- [x] 4.2 AgentView 将键盘 action、滚轮、滚动条和 turn 导航接到统一边界补页，限制同边界 in-flight；以真实输入分发测试验证三页连续浏览、失败重试及新跳转使旧请求失效（依赖 4.1，Tier 2）。
- [x] 4.3 Pager 时间线根据独立目录显示全局位置，点击远处目标接 around 并保留未加载间隙；验证同文本两目标、连点竞争、远跳两端续读与返回 live tail 不丢不重（依赖 3.2、3.3、4.1，Tier 2）。
- [x] 4.4 Minimal 渲染 owner 接入同一虚拟历史窗口并保护 native commit 高水位；在最近页已打印后补旧页、远跳并返回 live，PTY 验证无伪前插、无重复打印（依赖 4.1、4.2，Tier 2）。

## 5. 用量归属与结果留存

- [x] 5.1 chat-state 用量 owner 为结算补齐稳定 TurnId/prompt_id 或明确 session-only 归属（位置 index 仅核对），同步结算调用方和冲突校验；测试 rewind 后复用 prompt_index 不混账、实时与重放相同、迟到账单不归到最近 prompt、重复幂等且冲突拒绝（Tier 3）。
- [x] 5.2 chat-state 活动 Turn 用量在既有 working publication 留存并按有效 continuation 恢复，仅折叠未发布后缀；以主/aux/child 跨挂起、恢复后无新模型请求、已知 100+50=150 验证原 Turn 与 lifetime/segments 均准确，不增加全历史扫描（依赖 5.1，Tier 3）。
- [x] 5.3 Shell 结果 owner 保留恢复后完整 prompt usage 并通过原 TurnCompleted/完成索引有界查询；测试冻结结果跨分页/再次 resume 不变，session-only 后台结算只增加 lifetime，查询不重投 completion（依赖 5.2，Tier 2）。
- [x] 5.4 上下文 metadata owner 将 totalTokens 的上下文含义明确命名为 contextUsedTokens，同步 Shell/Pager/headless 调用方；测试压缩后 context 下降但 lifetime 按新结算上升，加载任意历史页均不改变消费（Tier 2）。
- [x] 5.5 streaming-messages-json 结果投影保留 completeness 与未知费用，不再填精确零；复用结果 usage 测试比较普通 JSON/ACP/该格式的 token、缓存未知、费用未知及部分账单语义（Tier 1）。

- [x] 5.6 用量展示 owner 利用已有运行时/报告完整性来源表达当前是已记录消费及仍有待结算工作；验证活动 child 与晚到 final bill 各自显示正确、终态只计一次，瞬时等待不污染持久 incomplete（Tier 2）。

## 6. 集成与文档

- [x] 6.1 执行跨进程 PTY 里程碑：非 Normal/各 Plan phase、多页历史、多来源消费、Ctrl+D、新进程 resume、滚轮补页、时间线远跳返程、原 Turn 完成与再次 resume；核对持久事实、UI 和所有结果用量。另测 resident reconnect 不新增 segment、不损坏 live owner，并覆盖 Minimal（依赖上述相关特性，Tier 3）。
- [x] 6.2 在已有长历史 fixture 上核对有界读页与首次可用边界，记录样本、页预算和查找工作量；确认状态/用量 hydration 无需正文全载，目录和 around 不逐页扫全历史（依赖 3、4、5，Tier 2）。
- [x] 6.3 同步 docs/development.md、Pager Plan/权限与用量说明，保存最终验证证据；运行 openspec validate --all --strict --no-interactive。仅在实际场景验收后勾选任务并归档，再运行全量和 archived 校验。
