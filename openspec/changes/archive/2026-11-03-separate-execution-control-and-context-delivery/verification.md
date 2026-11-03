# 方案验证

以下为分阶段验证历史，保留早期结论及当时未执行项。最终联合测试、夹具修正、独立 review、归档与 release 结果以 [收口记录](release-verification.md) 为准；不把历史待办解释为最终仍未完成。

## 本轮实际操作

- 阅读用户指定的本地 DSH checkout，记录 HEAD `ce7073d51254c26cd63b22a60835b8f948deb447`，核对 send/inject/claim/cancel 与持久恢复测试源码。未修改该仓库，未执行其测试。
- 核对当前 Grow interjection、sampling、notification、inquiry 和 Timeline 路径；相关当前源码优先于过期 Atlas 位置/窄搜索。Grow 用户 steer 会 request 软抢占，不能写成和 DSH steer 一样不取消推理。
- 只新增本目录中的 proposal、design、evidence、delta、tasks、review/验证记录；没有改动生产代码、主规范或旧 backlog，也没有编译/重建 release。
- 初次 `openspec validate separate-execution-control-and-context-delivery --strict --no-interactive` 失败：MODIFIED block 改了三项原有 scenario 标题，工具据此判为删除旧场景。已恢复原标题，在原场景中明确新语义；随后同命令通过。未删失败场景。
- `openspec validate --all --strict --no-interactive`：17/17 通过（含本 change、另一项既有 active change 和 15 项主规范）。
- `git diff --check` 与本 change 的本地 Markdown 链接核对通过；新增文件另做空白及链接校对。

## 审查边界

独立 reviewer 审查实际设计、delta 与相关当前源码；具体问题和复核状态见 [review.md](review.md)。设计审查不替代后续的真实 actor/provider、故障注入、fork/权限和持久化验证。

初审两项问题已关闭：新成功证明绑定原回答 Session/Sideband owner，防止 fork 重建他属待办；同步资格由新 typed proof 确定，历史 Answered 不回填。最终独立复核未发现剩余设计交付阻塞。补充只读 observation 不执行修复的场景后，主 agent 全量严格校验仍为 17/17，`git diff --check` 通过。

## 本轮消息路径分层复审

用户进一步明确：分层仅针对“消息投递”这种入口/方式内部的流程控制动作与上下文边界操作，其他上下文变更不归该方式管理。因此正式定义已收窄到 `mental-model.md`，其他机制的源码核对保留在 `surrounding-mechanisms.md`，仅作周边约束。proposal、design 和 tasks 已同步；未修改任何运行时代码。

对抗分析补充了两个可验证场景：final-close 前的 receipt 不能绕过取消/预算/Control 关闭后继执行资格；Consumed 后、请求发送前崩溃不能重复消费或声称模型已阅读。对应 delta 与后续测试任务已更新。独立复核见 [消息路径复审](mental-model-review.md)。

本轮最终全量 `openspec validate --all --strict --no-interactive` 为 **18 通过、1 失败（19 项）**，本 change 通过。另一个并行 change `analyze-multi-subagent-memory-growth` 尚无 delta 且未声明 skip_specs，未修改其文件；更早一次全量的另一失败项 `diagnose-ctrl-d-subagent-cancellation` 已由并行工作修正，本轮末次检查通过。上节 17/17 是前一轮记录，不代表当前全工作区全部通过。

`git diff --check`、本 change 全部 Markdown 本地链接与新增文件空白检查通过。仍未执行运行时测试或构建。

## 可运行 demo、修订与实现派发

主 agent 独立执行 `python3 openspec/changes/separate-execution-control-and-context-delivery/demo/model.py`，exit 0。最终模型深度7，144种初始配置，24,658个状态、98,953次转移、40类可达动作；16条重放场景与10个行为变异反例通过，正确模型未发现不变量违规。详细轨迹和界限见 [demo 说明](demo/README.md) 与其 results.json。不声称有限探索证明无限调度活性、多 writer 或 Rust 实现正确。

第一版较弱 demo 不作为验收：它只检查已发布输入没有越权，未检查是否漏掉 pending/terminal 义务，并有直接构造状态的场景。已改为同一 transition relation 的轨迹重放和实际修改转移行为的变异探索；补齐 terminal→Received 缺口、动态资格撤销、工具结算、两阶段最终决策及消费 ACK 不明。

由此作出的真实规划修订见 [planning-revisions.md](planning-revisions.md)。已建立 [实现任务书](implementation-task.md)，派发 `implement_passive_consultation` 执行首个完整闭环。此处只记录派发，运行时实现与测试结果由后续记录证明，不提前勾选生产/验收项。

## 当前收尾结果

首个增量已完成，独立代码审查 R1–R9 已关闭，未发现剩余阻塞问题。最终运行证据：Shell 4492 passed / 0 failed / 9 ignored；ChatState 610 passed / 0 failed / 3 ignored；四项真实消息旅程 4/4；Pager --tests 编译、fmt、diff 与 OpenSpec 全量 17/17 通过。精确命令、此前失败及修正、29 文件最终指纹和证据边界以 implementation-verification.md / independent-code-review.md 为准。

最新用户要求的实际使用分析列出 [50 类旅程](user-journey-audit.md)，区分已执行、源码/模型证据与未独立组合实测。长回答不可投递和失败原因丢失已修正；在途旧响应、咨询不等于前台决定、跨通知类别正文不保证全局排序、预算成本等边界仍明确保留。扩大联合测试的未执行项未被勾选，所以本 change 保持 active，未归档；未提交、安装或构建新 release。

所有本任务 Cargo 检查结束后，主 agent 检查当时没有 cargo/rustc 或相关测试进程，再执行 `cargo clean --profile dev -p shell -p chat-state -p pager`，退出 0。Cargo 报告移除 14,371 个开发产物、42.2 GiB 逻辑大小；这不是实测物理磁盘释放量。源码、日志、demo 与 release profile 保留，清理后没有再次启动无必要编译。
