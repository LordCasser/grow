## 1. 收尾所有权与结构化失败

- [x] 1.1 核对第一项的 typed observation 已可用，保留持久化错误的提交确定性、阶段及原事件身份至 runner；用错误映射测试证明 StorageFull、ACK lost、明确未提交和 integrity conflict 不被字符串合并。
- [x] 1.2 统一正常/error/panic/cancel 的 child 收尾入口，执行 outcome 与提交/交付状态分离；通过竞争测试验证单一 owner、已确认 outcome 不变且执行结束才释放执行额度。
- [x] 1.3 接入待收尾槽位、正文总字节上限及 admission 反压，固定并记录默认预算；通过容量边界与 completed-cache 淘汰测试验证未确认责任不丢失、引用不无限保留大正文。

## 2. 持久前缀与恢复

- [x] 2.1 建立 live/cold 共用的前缀核对与推进函数，覆盖 child scope/usage、output、child result、父用量、Ended 和 receipt；逐阶段故障注入验证重试只补缺失事实且用量不重复累计。
- [x] 2.2 接入失效 writer 的完整 drain、lease 重取及 exact event 核对；用 actor 先退出、持久任务未退出及 sync 后丢 ACK 的测试验证不能并发替换 writer 或改变原 payload。
- [x] 2.3 分离 terminal metadata 与 output availability，去除正文读取失败对已确认 outcome 的覆盖；测试文件缺失、摘要损坏、合法空输出和结构化输出，不能从最后 assistant 文本猜测结果。
- [x] 2.4 冷恢复先证明旧实例退出并恢复未闭合 scopes/结算，再生成有来源的 interrupted 结果；测试查询不可达、resident reconnect、未建成 child、未知用量与离线时长，不能伪造取消或完整零消费。

## 3. 等待与迟到交付

- [x] 3.1 为收尾和回执设置 caller 总期限、可取消 backoff 与每周期恢复额度；虚拟时钟验证 sender 存活不回复时有界退出、在途 mutation 不重复排入、重连不重置额度。
- [x] 3.2 将 FinalizationPending/OutputUnavailable 接入任务等待、输出和取消调用方，并用既有 CompletionDeliveryTracker/inbox 接管迟到结果；测试一个 call 只有一份 tool_result、receipt identity 去重以及父 writer 失败不虚报接纳。
- [x] 3.3 接入 Goal identity/revision 和既有 dismiss 规则；测试 Goal 停止、预算关闭、revision 变化时结果仍归属原 owner 而不自动续跑。
- [x] 3.4 接入 Workflow 未确认 child 调用和既有 journal 核对；测试恢复先查原 child、无绑定时停止自动推进、已退休 execution 不被迟到结果重启。

## 4. 故障矩阵与交付

- [x] 4.1 用注入式持久化 adapter 覆盖每个提交边界的写前失败、半条写入、同步前失败、同步后 ACK 丢失和永久不回复；断言一次 canonical 链、无重复 provider/tool、无虚假 terminal，不通过实际填满用户磁盘测试。
- [x] 4.2 用隔离临时存储和受控子进程覆盖关键边界退出/重启、lease 滞留、取消竞争及 worktree 保留；记录哪些后台进程拥有独立 owner，不能将 child 返回误判为全部进程停止。
- [x] 4.3 运行 chat-state/tools/shell/workflow 的受影响定向 `cargo test --locked -p <crate> --lib <filter>`，在 verification.md 逐项关联 Scenario 与实际测试结果；只在证据指向跨 crate 风险时扩大测试。
- [x] 4.4 更新 `docs/development.md` 收尾与恢复诊断说明，记录第三项 GC 保护尚未完成的边界；全量严格 OpenSpec 校验通过且所有场景落实后归档，运行全量与 archive 校验。
