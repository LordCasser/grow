# 压缩计数与状态转换审计

2026-10-29。结论：截图由计数差分失真、过早判断压缩后窗口、展示终态遗漏串联而成。手动指令执行了，历史没有被压成空上下文。另有与截图不同的确定控制流缺口：模型切换压缩失败后，自动压缩 suppression 能绕过普通请求的窗口检查。

本次只分析和登记，没有修改运行时代码或用户会话。前一轮 rewind/route rebase 修复尚不能视为整个 compaction 计数问题已解决。

## 会话事实

会话 `01a16811-f823-7a52-8b64-9a3ab105151a`；以下时间均为北京时间。seq 是 domain event 序号，presentation 另列 physical seq，避免两者混淆。

| 时间 | 事实 | 证据 |
| --- | --- | --- |
| 09:16:50 | DeepSeek 普通请求结算 input 714,227、output 278、total 714,505 | domain 29390–29392 |
| 09:18:07–09:18:47 | 手动压缩请求获得正常摘要；1057 项被一个 summary carrier 替换，总 Surface 1765 → 709 | domain 29402、29410–29413 |
| 09:18:47 | 展示通知明确记录 `manual=true, tokens_before=714505, tokens_after=0` | physical 41332 |
| 09:20:46 | 切到 neorouter/grok-4.7，Responses | domain 29418 |
| 09:21:10–09:21:11 | 接纳新输入；首 Step 关闭后开始预采样压缩，602,548 / 500,000 | domain 29423–29427，physical 41346 |
| 09:22:22–09:22:23 | 摘要请求成功，231 项替换成一个 summary carrier，Surface 710 → 480，压缩事务 Completed | domain 29433–29438 |
| 09:22:23 | Turn error：compaction converged but conversation still exceeds 500000 | domain 29441，physical 41364 |

最后一次 Started 与 Turn 失败之间没有 `AutoCompactCompleted`、`AutoCompactFailed` 或 `AutoCompactCancelled` presentation。该回合没有普通模型 Request Started，唯一模型调用是摘要 Sideband：所以这次截图的最后一条错误来自本地压缩后检查，不是普通请求被 provider 拒绝。

## 1. P1：原始历史差值扣实际请求压力，提前饱和破坏后续校正

入口：`chat-state/src/actor/mutations.rs:1125–1177`；请求估算口径在 `actor/state.rs:52–101`。

`projected_tokens` 是 provider 校准后的请求压力。Surface 则保留不会回放的 reasoning、旧 Goal 指令等。修改 Surface 时使用 raw bytes/4 差值，下一次 request projection 才撤销旧 raw→request 差值并安装新差值。这个算法依赖差值能完整抵消；中途 `saturating_sub` 将负余额丢弃，抵消就不再成立。

按 `estimate_item_tokens` 重建本次实际替换选区（文本 UTF-8 bytes/4，图片 765；核对所有 shadowed identity 及总 item count）：

| 阶段 | raw Surface | 其中 reasoning | 排除 reasoning 后的 Surface 估值 |
| --- | ---: | ---: | ---: |
| 手动压缩前，1765 项 | 1,316,195 | 823,667 | 492,528 |
| 手动压缩后，709 项 | 414,216 | 225,628 | 188,588 |
| 新输入后，710 项 | 414,759 | 225,628 | 189,131 |
| 后续自动压缩后，480 项 | 337,830 | 193,486 | 144,344 |

最后一列还没有应用完整 Goal/portable/工具定义等请求投影，不能当作精确请求 token 数。

手动压缩删除 raw 910,548，插入 raw 8,569：

```text
714,505 - 910,548 + 8,569 = -187,474
当前实现：saturating_sub → 0
```

这个负值只是混合坐标差分过程的中间余额，不代表“负上下文”。它必须在完整投影中抵消，不能先丢掉。后续再组装请求时，已经丢失的 187,474 会变成向上的计数偏差。若保持该历史路径的同一校准基线，602,548 去掉这一项为 415,074；这是差分校正对照，不是新模型 tokenizer 的测量值。

自动压缩继续按 raw 差值计算：

```text
602,548 - 86,259 + 9,330 = 525,619 > 500,000
525,619 - 187,474 = 338,145
```

338,145 同样只是保留旧校准的中间值，最终请求还需重新投影。足以确认不能拿现有 525,619 当作可靠的“确实放不下”证据。

另外，`apply_request_projection` 自身也分两次饱和。例如 P=3、旧/新 request input 都为10、旧/新 raw 都为0，重复相同投影得到10而非3。纯算术断言已运行；当前 Rust 回归没有证明混合坐标与饱和条件下的幂等性。

相同底层方法被 prune、image projection、integrity repair 和其他 replacement 复用，不能只给手动压缩显示值打补丁。普通 append 也暂按 raw 增量计数；后续 projection/有效 provider anchor 通常能校正，但不可依赖它恢复已丢失的负差额。

## 2. P1：压缩完成值与超窗判断使用尚未重建的投影

入口：`shell/src/session/actor/compaction.rs:1926–1969, 2042–2048`。

replacement 提交后重置了 native continuation，却立即读取 raw-delta 更新的 `projected_tokens` 并判断 `> context_window`。此时没有重新物化新 lane 的完整请求：portable reasoning、Goal shadows、工具 schema 和 envelope 尚未按新上下文对齐。

`run_manual_compact:665–681` 和同步自动成功通知也直接读取这一中间值，所以手动路径把 0 当作最终结果展示。异步路径已有“下一请求 projection 后才发布 tokens_after”的规范和实现，但公共 commit 的窗口检查仍发生得更早；异步完成通知的修复没有覆盖该检查。

仅把数字改成至少1，或者只把异步/手动通知延后，都不会解决错误的请求准入与收敛判断。

## 3. P1：切到较小窗口时，压缩抑制被当成允许继续采样

入口：`compaction.rs:2732–2772`、`2632–2657`；`turn/mod.rs:1766–1779, 1852–1885`。

针对用户给出的真实超窗场景，源码路径如下：

```text
600K / 1M → 切换到 500K
→ model-switch 触发压缩
→ 压缩失败或提交后仍超窗，设置 SUPPRESS_TURN
→ maybe_compact_on_model_switch 对非 auth Err 只记录日志，返回 Ok
→ 完整 request projection
→ check_auto_compact_needed 因 suppression 返回 None
→ provider admission 检查 owner / Step / storage，但没有通用输入窗口门槛
→ 请求可能进入 sampler/provider
```

因此“本轮不要反复压缩”与“这个请求能发送”被混在一起。普通 pre-sampling 的 forced compaction 失败会传播错误；model-switch 的特殊入口却吞掉错误，行为不一致。该问题由源码控制流确认，未运行新 Rust 集成 fixture，不冒充本次截图已经走了此分支。

边界：取消仍受 owner admission 阻挡，不能据此断言取消后仍会派发。same model slug 会跳过 model-switch 专用入口，但 suppression 为 NONE 时普通 projection 检查仍使用当前窗口，因此同 slug 本身不是必然绕过。带 task output budget 的子任务会跳过普通自动压缩，也应独立验证硬输入门槛，不能因自动压缩不适用就默认请求一定可发送。

## 4. P2：压缩事务完成、回合失败、UI 收尾被错误绑定

入口：`compaction.rs:983–1043, 1958–1963, 3083–3123`；Pager `app/acp_handler/session_notification.rs:2184–2230`、`app/session/mod.rs:2693–2702`。

Timeline 正确把已提交 replacement 记为 Completed，即使外层 Turn 随后失败。但 Grow 展示通知只在外层 Ok 时发 Completed；错误路径依赖 suppression 从 NONE 转移时发 Failed。converged-over-window 已提前写入 SUPPRESS_TURN，导致两条展示终态都不发送。

Pager 的 Started 同时设置 tracker activity 与独立 `live_feedback`。Turn 收尾清了前者，却没有清后者；只有压缩 Completed/Failed/Cancelled 会清 feedback。因此截图里的文字残留有完整日志和代码证据，并不表示摘要 provider 还在工作。

现有 `context_window_exceeded_converged_over_window_fails_turn` 回归检查了 Timeline Completed 和有界采样，但没有检查 gateway 的 Started→终态序列，故漏掉用户可见问题。

另一个相邻边界：manual/sync 成功分支才清 `pending_async_notice`。如果已有待发布异步通知，后来 replacement 已提交但 post-commit 流程返回 Err，旧通知可能在下一请求被冠以新 replacement 的结果。`context-compaction` 的 superseded pending display 契约要求 replacement 后丢弃旧通知；应补这一失败场景，而不是只覆盖 Ok。此项为源码发现，未在用户日志或新 fixture 中复现。

## 已有保护与设计限制

| 通路 | 已确认的现有保护 | 尚缺的边界 |
| --- | --- | --- |
| 冷恢复、rewind、路由替换 | 工作树已有 portable rebase；rewind 等 durable ACK 后再更新 | 不能替代 compaction/prune 的同坐标重估；历史运行版本未确认已包含该改动 |
| 普通采样前 | 先组装完整请求，再按压力触发 forced compaction | suppression 可跳过检查；需分开自动压缩策略与硬输入准入 |
| 工具结果追加后 | 有 preflight overflow 检查 | 仍读取同一压力计数并受 suppression 影响 |
| provider 明确 overflow | 有独立恢复判据及重复 overflow 停止规则 | 不应以无界重试代替正确的本地准入 |
| 后台生成与 promotion | 冻结输入、边界发布、authority/route 复核、取消后丢弃迟到结果 | commit 的中间压力判断、promotion 的展示终态仍须覆盖 |
| 压缩选区 | 完整工具往返边界；预算缩小真实选区，不删除未摘要历史；保留尾部 | 单个有界摘要未必把总上下文压进新窗口 |
| provider usage 校准 | 用量与上下文压力分账，低于启发式下界的锚点被拒绝 | 这是现有保守规则和测试；未证明它导致本截图，不应顺手重写用量账本 |

选区先按窗口的16%保留尾部，再把摘要来源限制在新模型的输入预算内。来源过大时只摘要能装下的完整前缀，剩余历史原样保留。当前规范明确允许“一次 replacement 后仍超窗就结束本轮”，所以缺少多段压缩不是背离现行规范的实现回归；若要让更大的真实上下文自动适配小窗口，需要单独改变这一契约。

对 600K → 500K，不能直接把600K全文塞给500K摘要模型，也不能假设一次压缩一定不够。应先得到真实请求估值、选择合法有界来源、提交并重新估值：能发送就继续；有剩余合法范围且确实缩小才允许继续有界压缩；没有进展、不可压缩的必需内容或输出预留本身超限就明确终止。不得用重复相同来源或删未摘要内容来“收敛”。

## 建议实施顺序与验收

先修统一计数与请求投影，再修硬窗口准入和错误传播，最后统一压缩终态发布。复用现有 ChatState、请求组装、compaction transaction 与通知入口，无需新增另一套状态机。

1. 压力始终基于同一有效请求坐标；provider 校准与估算绑定同一路由/投影基线。raw 历史用于审计和选区，不直接当成已回放 token 增减。仅把 u64 换成有符号数不够：可以恢复代数，但中间值仍不能用于显示或收敛判断。
2. replacement 后，在完整请求投影可用时决定最终 tokens_after 和是否容纳当前请求；manual 没有后继普通请求时也要获得同口径的本地估算，不为显示数字额外发模型请求。
3. 硬输入准入与自动压缩的阈值/suppression 分离；失败不能授权发送明确超窗请求。传播 fatal persistence/post-commit authority 错误，保持取消 owner 边界。
4. 每个展示 Started 恰有一个匹配终态。已提交摘要但回合不能继续时，分别表达“压缩已提交”和“回合失败”；结束反馈不等待下一条用户消息。旧 pending notice 在新的 replacement 提交时失效。

必须补的对抗性验证：

- reasoning 占多数，manual raw 减少量大于当前压力：不显示伪0；连续重建相同请求不增长。
- 同一路由的 compaction、prune、image projection、repair，与换路由/rewind 的交错；旧校准不跨新 route，ACK 失败不发布新状态。
- 600K/1M 切500K：压缩成功后才采样；size/auth/persistence/取消/仍超窗分别收尾；明确超窗时普通 provider 请求数为0。
- same slug 不同窗口、两次连续切换、queued switch 在 Step 边界应用；检查使用最终配置与请求投影。
- 一次有界摘要仍不够、无法形成完整来源、必需 tail 太大、输出预留太大；不拆工具对、不吞新输入、不重复相同失败恢复。
- manual/sync/async/promotion 的 committed+error，以及 Started→Turn error；核对 Timeline、gateway、Pager live feedback 三层，而非只断言 Timeline Completed。
- pending async notice 后发生 committed+error 的新 replacement；不能把新 token 数记到旧事务。

## 验证范围

已执行只读事件核对、选区 identity/数量重建及纯算术断言，并由两个 Luna 子任务分别复核计数与 UI/模型切换控制流。没有运行新 Rust 回归、真实 provider 复现或 UI 自动化；不将源码推演称作通过集成测试。没有构建 Cargo 产物，也没有清理或改写用户正在使用的 session。OpenSpec 校验记录见 verification.md。
