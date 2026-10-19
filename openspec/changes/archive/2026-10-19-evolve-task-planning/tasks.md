## 1. Proposal preparation

- [x] 1.1 核对现有 TODO、task/嵌套路由、消息、Goal、结果持久化与 UI/压缩调用方，记录当前事实和差异。
- [x] 1.2 完成 proposal/design/delta specs，覆盖弱关联触发、失败竞争与周边交互，建立验收场景映射。
- [x] 1.3 进行对抗性文档审查，修复冲突并执行 change 及全量 strict 校验；记录未实现边界。

## 2. Local plan authority and links

- [x] 2.1 实现 SessionActor 的局部计划类型化入口及 Timeline fold，TODO Resources 改为派生；验证 TP-01—03、ST-01—03 的持久失败与重建。
- [x] 2.2 为 task 加入可选父步骤引用并验证 session/范围/owner，和父项修改排序；覆盖 TP-04—05、TA-01。
- [x] 2.3 实现有效差异表、删除 tombstone、replace 与范围切换的责任保留；tool-types106 与 Shell111 具名测试分层覆盖 TP-06—09、TP-16、TP-19。证据和仍未达成的整行验收边界见 [verification.md](verification.md)。

## 3. Child-owned replanning

- [x] 3.1 复用 parent inbox 实现来源交付责任、稳定批次、覆盖/去重与确认交接；故障注入覆盖 TP-14—15、ST-01—05、ST-13—15。
- [x] 3.2 实现 child 局部变更/no-change 与 Handled 原子提交；覆盖 TP-10—13、TA-02—04；测试等待可中断但外部命令不被自动杀掉。
- [x] 3.3 实现终态不可应用、直接边逐级传播和责任收敛；覆盖 TP-17—18、TP-32、CS-03、ST-12、ST-15，验证无复活/通知环及删除不隐式 kill。

## 4. Waiting, delivery and completion

- [x] 4.1 分离 root lifecycle owner 与直接父级交付目的地，复用 canonical result/receipt；集成测试覆盖 ST-06—08、ST-16，含逐级验收摘要、nested waiter/通知竞态及 Workflow 内部执行回归。
- [x] 4.2 实现结构化等待、真实身份/any-all 校验和无丢唤醒注册；覆盖 TP-20—22，包含失败、中断与不存在依赖。
- [x] 4.3 实现普通任务/child 的等待非终态与同 owner 继续；覆盖 TP-23—25、TP-31，确认未新建 Goal、多次唤醒未重置预算/max-turns、停止后不重启。
- [x] 4.4 同时修改 Goal 内层准入与外层 idle 重排；用 mock provider 请求计数和 actor 唤醒计数验证 BG-01—03、BG-09，无模型轮询也无 host 忙循环，等待不绕过原 gate。
- [x] 4.5 实现必需交付验收/要求变化复核、义务处置和全目标完成检查；覆盖 TP-26—30、BG-04—08，不引入结果版本硬门禁或所有 TODO 全局硬门槛。

## 5. Projections, context and recovery

- [x] 5.1 更新 Grow/ACP、Pager/headless 投影与 child 计划查看，移除 turn-end 假完成；验证 CS-01—04 的 live/resume/replay 一致性。
- [x] 5.2 更新模型提醒、异步压缩边界和引用保留；覆盖 CC-01—04，确认 consumed 不等于 handled 且无需整树注入。
- [x] 5.3 接入既有保留/清理与冷恢复规则；覆盖 ST-11—13，历史读取不得修复或执行。
- [x] 5.4 在 `suspend-session-tree` 实现后联测同身份树恢复、待重规划/等待/验收与中断命令；覆盖 ST-09—10，缺少依赖时明确未执行。

## 6. Verification and archive

- [x] 6.1 按 acceptance.md 运行所有相关单元、actor/coordinator 集成、故障注入与 UI/headless 场景；逐项记录实际命令、结果及限制。
- [x] 6.2 执行针对最终实现的对抗性审查与相关回归，修复失败；同步更新 `docs/development.md`、`docs/architecture/agent-core-timeline.md`、工具说明及适用客户端说明。
- [x] 6.3 实现验证完成后执行 `openspec validate --all --strict --no-interactive`，核对场景与证据后才归档；归档后检查主规范、索引与全量校验。


最终运行证据及分层验收范围见 [verification.md](verification.md) 文首；已完成归档；归档后全量与 archive 校验记录见版本联合验证。
