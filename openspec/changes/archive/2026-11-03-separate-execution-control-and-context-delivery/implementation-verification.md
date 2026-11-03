# 实现验证记录

> 本文保留该阶段的证据与缺口。最后联合测试和收口状态以 [release-verification.md](release-verification.md)、[joint-task-evidence.md](joint-task-evidence.md) 为准。

状态：首个端到端实现及其最终代码验证已完成，等待主 agent 对实际旅程覆盖和独立 review 收尾；本记录不替代主 agent 的最终完成结论。生产实现与任务基线的精确 diff 以 `/tmp/grow-passive-coder-baseline` 对照当前文件，未 reset/revert 工作区前序改动。

## 实现

- 成功 Incoming audit 保存原回答 Session 与 `SidebandRun::complete` 返回的精确 Result 引用，公开 `InquiryOutcome` wire 形状未扩张。纯 `inquiry_prompt` 复用静态 `DEFAULT_REMINDER_TAG`，供 live 请求与 cold 验证逐字核对。
- `InquiryAnswered` 是 Session-owned context-only source。稳定 receipt identity 按原回答 owner、提问 Session 和 inquiry ID 生成；版本绑定原成功 terminal 的 `(Session, EventSeq)`。不会以第二个 terminal seq 分配第二份问答收据。
- 原 writer 用 canonical Timeline suffix + 单调非权威 cursor 协调缺失 Received；游标只在捕获前沿完成后推进。成功后的同步失败不改写 Answered。startup、既有 outcome retry 与真实消费边界均经过此路径。
- 严格 loader 校验原 target、InfoRequest spawn 在 terminal 之前、完整 Sideband grammar/Result、精确问题及 source/cwd、最终公开答案、terminal 唯一性，以及每条 Received 的 source/version/owner/正文 hash。旧无证明审计与 fork 他属事实不产生新义务。
- final response 只消费在控制栅栏内捕获的候选 IDs；后到消息不倒灌旧候选。payload I/O 后在消费准入处重新检查 exact Turn/foreground、terminal preemption、Goal provider gate 与 Behavior，失败保留 pending。
- 通知读取/Consumed 提交错误传到真实 first/successor/final 请求边界，不再等同于 false；新 source 不允许 `Consumed{input:None}`。普通 source 的既有 input=None 语义保留。
- Pager 只有两处 audit struct 测试 fixture 的新增 optional 字段同步，没有 Pager 生产行为修改。开发者架构说明已同步；主规范留待完成验证后由 root 归档合入。

## 实际已执行的阶段验证

- `cargo check --locked -p chat-state -j 4` 通过（初始来源/后缀查询增量，26.79 秒）。
- `cargo check --locked -p shell -j 4` 通过（初始生产贯通状态）。随后新增 provenance 与消费约束，最终 source 状态以下面的最终 run 为准。
- 早期 test compile 暴露新增 proof/cursor 字段的 fixture 漏改和测试配置类型错误，均已修正；没有放宽生产校验。
- 早期 `cargo test --locked -p shell --lib passive_ -j 4`：3/3 通过，覆盖 idle no-wake、候选精确选择/late pending、真实 handle_prompt payload 失败禁止 provider。
- 扩展真实 SamplerActor 后同 filter：4 通过、1 fixture 失败。失败发生于 TurnStarted（输入 Submitted 写在新 ChatState、EventTracker 仍指向旧 handle），尚未到故意注入的 Consumed 失败。已在 fixture 重建 EventTracker，保留严格的 notification-consumption phase 与请求数断言，未把任意 fatal 当验收。

## 验证过程与失败修正

- 第一轮全量只跑完 ChatState，609 通过、1 新测试 fixture 失败、3 忽略：新增 passive 混合消费测试漏设 prompt_index；修正 fixture 后保持原生产校验。
- 第二轮（`/tmp/grow-passive-full-libs-v2.log`）ChatState 610 通过、0 失败、3 忽略；Shell 4488 通过、2 失败、9 忽略。失败分别为未修改的 config watcher 等待文件事件超时，以及新真实 Turn 存储故障测试发现的原错误被 input_terminal 清理失败替换。
- config watcher 源码与本任务基线逐文件相同；同环境隔离 exact 测试 1/1 通过（6.12 秒，`/tmp/grow-passive-watcher-isolated.log`）。这支持与并行/负载有关的判断，未确认超时根因，也未修改 watcher 或放宽期限。
- 原错误丢失是共享失败路径的真实问题：Consumed 的 StorageFull 令 writer fail-stop，后续输入清理和 Turn 结算失败覆盖了最初的 notification-consumption 错误。已在既有 input_terminal fatal 构造处保留原 execution Err 为 nested cause，与 terminal 既有错误包装方式一致。没有改动错误控制流或新增框架。
- 修复后严格定向测试 `passive_consumption_failure_blocks_first_successor_and_final_candidate_requests` 1/1 通过（内部依次走 first、tool-result successor、final candidate 三条路径；0.58 秒，`/tmp/grow-passive-fault-focused.log`）。仍要求实际 notification-consumption 原因及真实 HTTP 请求数，未放宽为任意 fatal。
- `cargo fmt --all -- --check` 与 `git diff --check` 均退出 0。当前 task 基线对比的 29 个代码/文档文件 SHA256 记录在 `/tmp/grow-passive-final-source-manifest.json`，生产源冻结于上述 cause 修复之后。

第三轮全量（R1–R8 修正后的冻结源码）：`GROW_HOME=/tmp/grow-passive-test-home RUST_MIN_STACK=16777216 cargo test --locked -p shell -p chat-state --lib -j 4 -- --test-threads=2`，退出 0。ChatState **610 passed / 0 failed / 3 ignored**（20.23 秒），Shell **4490 passed / 0 failed / 9 ignored**（394.50 秒），包括前轮 watcher。日志 `/tmp/grow-passive-full-libs-v3.log`。该构建早于下面新增的 R9，不冒称是 R9 后的最终源码。

## 实际旅程补充与 R9

- 独立旅程审查发现“有效长回答先成功、永久卡住通知”的真实边界：完整问答正文含题目与来源 header，而已有通知上限为 1 MiB。修正后先完成并保留 Sideband Result/Completed End 和 usage，再用共享 renderer 生成候选完整正文，按现有上限判断；不可接纳时返回 typed Failed，不发布成功 proof、不截断、不创建重试墙。冷 validator 同样拒绝无 Received 的超限新证明，复用已计算正文，无新 I/O。
- 新 joined 测试走实际 `enqueue_coordination_inquiry → InfoRequest → proof → Received`，同时让真实 `handle_prompt` sampler 请求保持阻塞；收据接纳不改变在途 Surface、不取消采样，释放后同一 Turn 的真实后继请求包含完整归属问答，且仅消费一次。再次提交同 inquiry 不增加请求/收据。新的 consultation 输入明确存在并无 PermissionEvidence。
- 真实 cap 测试以实际小 InfoRequest 的证明元数据测得 header 字节数，包含中文题目和多字节答案；完整正文恰好 1 MiB 可接纳，增加 1 字节变为 Failed/无 proof/无新收据，已完成 Sideband 的完整 raw Result 仍在。冷负例同时保持问题与 Result 一致，只把完整正文增至超限，并且没有 Received，明确要求 payload-limit 错误。原始答案为 1 MiB 的反例失败后，回答方下一真实用户 Turn 仍成功派发并完成。
- `/tmp/grow-passive-message-delivery-final.log`：最终 R9 生产源码的 **4 passed / 0 failed**（1.16 秒），包含 joined、cap/冷负例、blocked sampler 和实际 first/successor/final 消费故障的严格断言。最初 cap 测试有字符串拼接与 Result 字段名的测试编译错误，已修正，未放宽断言。
- `/tmp/grow-passive-joined.log` 另有 R9 之前 joined 定向 **1 passed**；`/tmp/grow-passive-cap-v2.log` 有 cold-cap 校验加入前的 cap 定向 **1 passed**。它们只作阶段记录，最终以上述 4-test run 为准。

最终 R9 后的 Shell 全量：`GROW_HOME=/tmp/grow-passive-test-home RUST_MIN_STACK=16777216 cargo test --locked -p shell --lib -j 4 -- --test-threads=2`，退出 0，**4492 passed / 0 failed / 9 ignored**（406.05 秒），日志 `/tmp/grow-passive-full-shell-final.log`。ChatState 在第三轮 610 passed / 3 ignored 之后没有源码变化。随后按顺序执行 Pager 调用方检查：`GROW_HOME=/tmp/grow-passive-test-home RUST_MIN_STACK=16777216 cargo check --locked -p pager --tests -j 4`，退出 0（3 分 41 秒），日志 `/tmp/grow-passive-pager-check.log`。

`openspec validate --all --strict --no-interactive` 退出 0，17 passed / 0 failed，日志 `/tmp/grow-passive-openspec-final.log`。编译仍有既有 `Atomic::fetch_update` 弃用提示与大测试二进制 linker unwind 提示，未顺带修改。

当前代码/文档 29 个变更文件的 SHA256 manifest 已按最终 R9 源码重新生成：`/tmp/grow-passive-final-source-manifest.json`。`cargo fmt --all -- --check` 和 `git diff --check` 通过。构建使用现有 target、低并发和独立 GROW_HOME；遇到共享 build/artifact 锁时等待，未结束其他任务进程。之前一次短名称搭配 `--exact` 的命令只选择了 0 测试，未计入验证。

## 证据范围

新增运行时测试涵盖实际排队 Sideband 与真实前台/后继 provider 串联、idle no-wake、精确候选选择和 late pending、三条消费失败阻断、mandatory input、canonical proof/receipt 错配、duplicate/forward refs、terminal-without-Received 前缀恢复、关闭执行 gate 延后协调和 fork skip。已有全量测试同时覆盖原 steer、Goal/Plan/Workflow、工具结算和挂起恢复机制。未为每一种 source × 每一个执行状态重做全部交叉组合；actual blocked-tool +真实咨询、全部三种调用方向的 source-side 工具链、完整进程重启+新咨询组合没有逐项新增独立端到端测试。它们由共享入口、已有控制回归、冷验证与有限模型提供不同层面的证据，不能冒称全部组合都经过 provider 实测。

## 交接状态

- 实现任务 2.1–2.4 已按最终验证勾选。第 3 节列出的全部交叉旅程没有虚称逐项实测完成，留给主 agent 按独立旅程矩阵与本记录收尾；这里没有新增产品 backlog 或默默延后功能行为。
- 生产源、测试与开发者说明的最终 29 文件 manifest 在全部验证后再次校验，无 hash 差异。源码基线 `/tmp/grow-passive-coder-baseline/` 保留供独立 diff，不覆盖工作区前序改动。
- 没有执行 commit、archive、release build、安装或发布。其他任务的 Cargo 进程未被终止。测试使用既有 target；共享缓存的后续清理由主 agent 协调，不能在并行任务仍构建时删除。
