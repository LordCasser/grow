# Sampling / model data 分区审计

## 基线与范围

审计基线为 HEAD `d6128138cacdc22553aba9dafba9bc6cc934e4d1` 加开始审查时已有工作树，报告只写当前分区。根目录 `openspec/changes/audit-grow-architecture-across-versions/inventory.md` 记录全局发布身份：本地与 `origin` 均有 v1.0.0–v2.6.0 共 46 个 tag，无远端独有 tag；HEAD 在 v2.6.0 tag 之后。模型工具、resume 和相邻 session 改动仍在并行开发，相关交界处留待最终复核。

覆盖模块：

- `crates/codegen/{sampler,sampling-types,token-estimation,grow-http,auth}`
- `crates/common/compaction`
- 只读 shell/chat-state/memory 消费边界：sampling actor 创建与事件排空、turn sampling、summary/session compaction、memory flush、chat-state token 阈值及 memory embedding HTTP/auth 调用。

核对的当前契约为 `openspec/specs/model-sampling/spec.md` 和 `openspec/specs/context-compaction/spec.md`；前者有并行工作树修改，因此涉及进行中特性的场景不当作已定稿依据。

## 当前职责与执行链

- `SamplerHandle` 将提交命令送入 provider-neutral sampler actor。actor 在接纳请求时复制默认配置/retry policy，显式 per-request config 覆盖该快照；后续 `update_config` 只影响之后的默认配置请求。每个任务使用共享 `RecoveryBudget` 做 attempt/deadline/replay-safe 准入。
- `submit_and_collect_accounted` 用 drop guard 将调用方 future 丢弃转换成取消；`run_request_task` 在 provider attempt 前捕获 shell 提供的 scope，运行流协议，等待 evidence/usage sink 确认后才重试或发终态。shell 的 session event drainer 将有序事件投影到会话；shell 持有 Goal/session/persistence 语义，sampler 不直接依赖这些概念。
- compaction 的 range 选择、会话冻结、Sideband/Goal usage 与 durable replacement 由 shell owner 管理；`common/compaction` 提供 `CompactionSampler`、摘要 prompt/retry、失败分类、tool-result prune 等 transport-agnostic 能力。`ShellCompactionSampler` 在 Sideband 和适用的 Goal accounting 边界结算每次 attempt。
- `sampling-types` 提供 provider-neutral conversation 和 compaction bridge。`token-estimation` 是无外部依赖的算术 helper，真实消费者包括 compaction、memory flush、chat-state。`auth` 提供 `HttpAuth` / BYOK credential snapshot seam；`grow-http` 提供共享 HTTP client/UA 与相关 middleware，memory embedding 是核对过的实际消费者。

## 当前发现

### SAMPLER-1 — 按 request ID 清理会误删替代请求，panic 时也会遗留伪活动项（P2）

**触发与机制。** `SamplerActor::handle_command` 接到重复 `RequestId` 时，用 `ActorState::register` 替换 map 项，再取消旧 token。两个 JoinSet task 都只返回同一个公开 `RequestId`。旧 task 在新项注册后结束，`SamplerActor::run` 的 `join_next` 分支会按 ID 无条件 `remove`，因而能删除新 task 的 token；之后发给新请求的 `cancel` 不再能取消 provider 工作，`is_active`/`active_count` 也报错。任务 panic/abort 走 `JoinError` 分支时只记录日志、不移除对应活动项，反向留下永不完成的 active 记录。前一种情况要求请求 ID 重用；shell turn 当前用 `RequestId::random()`，所以该路径在已审 shell caller 中不常见，但公开类型明确允许外部传入 ID，actor 也显式实现了重复 ID 处理。

**证据。** [actor/mod.rs](../../../../../crates/codegen/sampler/src/actor/mod.rs#L156) 的 JoinSet 正常完成分支只按 ID 删除；同文件 [重复注册处理](../../../../../crates/codegen/sampler/src/actor/mod.rs#L222) 覆盖 map 后取消旧 token。[state.rs](../../../../../crates/codegen/sampler/src/actor/state.rs#L40) 将 active map 键设为 `RequestId`，`remove` 无任务代际条件；[RequestId 文档](../../../../../crates/codegen/sampler/src/types.rs#L7) 允许调用方提供外部 ID。shell 当前请求入口在 `turn/sampling.rs::run_turn_via_sampler` 调 `RequestId::random()`（约 L2105）。测试覆盖 state 层重复注册返回旧项，以及 actor 正常取消、并发不同 ID 和 shutdown settlement；未见旧任务晚完成时保留替代项或 panic 后清理的 actor 生命周期回归测试。

**受损不变量与后果。** active map 必须代表当前注册代际，而且一个 task 的完成只能结束自己的登记。违反时，重复 ID 的新 provider 请求可失去取消控制并在调用方放弃后继续消耗/结算；panic 则会让已终止 task 长期占据活动状态，误导查询与监控。

**所有者与可证伪通过标准。** 由 sampler actor owner 修复/复核。增加确定性 actor 级测试：A 注册后用同一 ID 注册 B；A 被取消并在 B 注册之后完成；B 仍为 active，随后对该 ID 的 cancel 确实停止 B，且 attempt settlement 只结算一次。另强制一个 request task panic，确认只清理其登记且 actor 可继续服务；不得删掉另一个 task 的活动项。可通过任务代际/任务身份关联清理，或拒绝重复 ID，但完成和 panic 必须同样可验证。

## 已确认的历史修复

### Sideband 压缩保留工具证据：v2.1.9 之后已修复

只读 ref 核对发现 `origin/codex/pre-release-v2.1.10-local` 有两个相对 main 独有提交：`ae1f406a8`（`fix: preserve tool evidence across sidebands`）和 `5ef50112d`（release identity）。原修复父提交为 v2.1.9。main 后续以不同提交 `48dbcf24c` 收录同一修复；对比原修复触及的 43 条路径，v2.1.10、v2.1.11 与 main 修复输出均有 42 条文件 blob 完全一致，唯一差异是之后继续演进的 `openspec/specs/model-sampling/spec.md`。检查的 conversation、compaction、coordination 与 summary 实现文件在 v2.1.10 起与修复结果一致；v2.1.9 对应源码 blob 仍不同。故这是 v2.1.10 已发布的历史修复，不是当前 mainline 遗漏项，也不应作为现存缺陷重复登记。

## 46 个 tag 的分区版本表

下列 `模块树集 fp` 是审计脚本对 `git ls-tree` 取得的 6 个逻辑 crate（含 v1 旧路径映射）的 `(逻辑 crate, 实际路径, 对象类型, tree OID)` 列表做稳定序列化后取 SHA-1 前 8 位；`消费文件集 fp` 是 10 个已选 shell/chat-state/memory 消费入口对应 blob 集的同类指纹。它们是**比较指纹，不是 Git tree OID**。完整 tag commit 与日期见全局 inventory。相邻行两项指纹相同，表示已核对的模块根树和选定消费文件均可复用；未改变并不代表该 tag 的其他仓库部分无变化。

| Tag | 模块树集 fp | 消费文件集 fp |
| --- | --- | --- |
| v1.0.0 | `da393f89` | `ac563c65` |
| v1.1.0 | `514c87cc` | `616d3ab9` |
| v1.1.1 | `514c87cc` | `616d3ab9` |
| v1.1.2 | `514c87cc` | `616d3ab9` |
| v1.1.3 | `514c87cc` | `0d00c4a1` |
| v1.1.4 | `5286a1ad` | `8b6efd50` |
| v1.1.5 | `214def10` | `8b6efd50` |
| v1.1.6 | `214def10` | `8b6efd50` |
| v1.1.7 | `214def10` | `8b6efd50` |
| v1.1.8 | `3a05612f` | `8b6efd50` |
| v1.1.9 | `3a05612f` | `8b6efd50` |
| v1.1.10 | `3a05612f` | `8b6efd50` |
| v1.1.11 | `3a05612f` | `8b6efd50` |
| v2.0.0 | `5b9ac42f` | `098e6729` |
| v2.0.1 | `60067e67` | `220bf1aa` |
| v2.0.2 | `093ce6d6` | `3c094707` |
| v2.0.3 | `f7543aee` | `b3d70e0e` |
| v2.0.4 | `f7543aee` | `b8686544` |
| v2.1.0 | `b0916869` | `4751a9d2` |
| v2.1.1 | `b0916869` | `8ce315d6` |
| v2.1.2 | `3b88ffd9` | `674eb25b` |
| v2.1.3 | `1b8d8bd9` | `17fe0810` |
| v2.1.4 | `d438442e` | `495d2f58` |
| v2.1.5 | `e3e0c8da` | `1fb60b44` |
| v2.1.6 | `a7c0bf49` | `3abba0c3` |
| v2.1.7 | `46f69f19` | `9f0f14af` |
| v2.1.8 | `46f69f19` | `9f0f14af` |
| v2.1.9 | `46f69f19` | `9f0f14af` |
| v2.1.10 | `35d4d2bb` | `4dd5031b` |
| v2.1.11 | `35d4d2bb` | `4dd5031b` |
| v2.2.0 | `4cf0ba0a` | `6c9c89e4` |
| v2.2.1 | `4cf0ba0a` | `f5d46908` |
| v2.2.2 | `bbfe2565` | `6652411d` |
| v2.3.0 | `5dc89a65` | `6652411d` |
| v2.3.1 | `5dc89a65` | `6652411d` |
| v2.4.0 | `bd2feeb1` | `7632a296` |
| v2.4.1 | `20c972bc` | `7bd9880f` |
| v2.4.2 | `20c972bc` | `c568479d` |
| v2.4.3 | `20c972bc` | `c568479d` |
| v2.5.0 | `bd8520b1` | `31101bdd` |
| v2.5.1 | `bd8520b1` | `31101bdd` |
| v2.5.2 | `3ba7f64a` | `e17558a7` |
| v2.5.3 | `3ba7f64a` | `e17558a7` |
| v2.5.4 | `3ba7f64a` | `e17558a7` |
| v2.5.5 | `b9ae96af` | `a52cf04a` |
| v2.6.0 | `7ac8e1b3` | `11fbfbf1` |

相邻 tag 的所有非零变化点（crate 名均指逻辑模块；消费数是上述 10 个文件中变更的文件数）：

| 转换 | 模块树变化 | 消费入口变化 |
| --- | --- | ---: |
| v1.0.0 → v1.1.0 | 六个模块路径迁移/树变化；`token-estimation` 新出现 | 5 |
| v1.1.2 → v1.1.3 | — | 1 |
| v1.1.3 → v1.1.4 | sampler、sampling-types、grow-http、auth | 1 |
| v1.1.4 → v1.1.5 | sampling-types | 0 |
| v1.1.7 → v1.1.8 | sampler、sampling-types、compaction | 0 |
| v1.1.11 → v2.0.0 | sampler、sampling-types、grow-http、compaction | 9 |
| v2.0.0 → v2.0.1 | sampler、sampling-types | 5 |
| v2.0.1 → v2.0.2 | sampler、sampling-types | 3 |
| v2.0.2 → v2.0.3 | sampling-types、compaction | 3 |
| v2.0.3 → v2.0.4 | — | 1 |
| v2.0.4 → v2.1.0 | sampler、sampling-types | 6 |
| v2.1.0 → v2.1.1 | — | 1 |
| v2.1.1 → v2.1.2 | sampler、compaction | 4 |
| v2.1.2 → v2.1.3 | sampler、sampling-types | 4 |
| v2.1.3 → v2.1.4 | sampler、sampling-types | 6 |
| v2.1.4 → v2.1.5 | sampler、sampling-types | 6 |
| v2.1.5 → v2.1.6 | sampler、sampling-types | 6 |
| v2.1.6 → v2.1.7 | sampler、sampling-types、grow-http | 3 |
| v2.1.9 → v2.1.10 | sampler、sampling-types | 3 |
| v2.1.11 → v2.2.0 | sampler、sampling-types | 6 |
| v2.2.0 → v2.2.1 | — | 1 |
| v2.2.1 → v2.2.2 | sampler、sampling-types | 2 |
| v2.2.2 → v2.3.0 | sampler | 0 |
| v2.3.1 → v2.4.0 | sampler、sampling-types | 4 |
| v2.4.0 → v2.4.1 | sampling-types | 3 |
| v2.4.1 → v2.4.2 | — | 1 |
| v2.4.3 → v2.5.0 | sampler、sampling-types、auth | 3 |
| v2.5.1 → v2.5.2 | compaction | 5 |
| v2.5.4 → v2.5.5 | sampler、sampling-types | 3 |
| v2.5.5 → v2.6.0 | sampler、sampling-types、compaction | 4 |

未列出的相邻转换，其模块树集与选定消费文件集均一致。最早路径迁移按旧的 `grow-*` 路径解析；未将路径缺失误记为模块删除。v1.1.0 至 v1.1.3、v1.1.5 至 v1.1.7、v1.1.8 至 v1.1.11、v2.0.3 至 v2.0.4、v2.1.0 至 v2.1.1、v2.1.7 至 v2.1.9、v2.1.10 至 v2.1.11、v2.2.0 至 v2.2.1、v2.3.0 至 v2.3.1、v2.4.1 至 v2.4.3、v2.5.0 至 v2.5.1、v2.5.2 至 v2.5.4 已按相同指纹复用所选树/入口的证据。

## 覆盖边界与验证记录

- 版本审查以六个逻辑 crate 根 tree OID、指定消费文件 blob、相邻 tag 差异和抽样源码历史为证据；不是逐行审完 46 个 tag，也没有在每个旧 tag 上构建全部平台。v1.0.0→v1.1.0、v1.1.x→v2.0.0、v2.1.x 与 v2.5.x→v2.6.0 的大变化单独核对了分区相关文件。
- 当前检查确认 sampler 的 evidence/usage ACK、取消、retry admission 和 shutdown join 有相应定向单测；compaction 的 bounded attempts、失败分类、空/退化摘要与 shell durable settlement 有相应实现和场景。静态检查未发现 auth/grow-http/token-estimation/compaction 其他足以确立的现存架构缺陷；这不是零缺陷保证。
- `common/compaction/src/code_compaction/sample.rs` 在 transient attempt 后使用普通 `sleep(retry_delay)`。shell compaction 目前设置 3 秒、最多 3 次尝试，取消在该 sleep 结束后由下一次 sampler call/外层 owner 观察；已读到的契约要求取消后保留 settlement/publish 屏障，但没有更低的取消响应时限。本次不将这段有界 backoff 单独列为缺陷；若上层要求小于 3 秒的取消确认，需单独做定向时序验证。
- 未修改或运行测试、构建、OpenSpec validate；结论为静态代码与 Git 历史审查。模型工具/resume 并行修改相关区域待最终工作树状态稳定后复核。
