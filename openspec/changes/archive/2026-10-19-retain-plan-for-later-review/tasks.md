## 1. Proposal

- [x] 1.1 核对已有四阶段、artifact、审批 CAS、idle resume/handoff 和 Pager 读取差异，形成 proposal/design/spec delta。
- [x] 1.2 完成严格 OpenSpec 文档校验并记录 verification。

## 2. Canonical read and retained candidate — completed

- [x] 2.0 先核对既有四阶段、artifact helper、Control/CAS、恢复审批及 handoff 测试作为复用基线；新增只覆盖候选资格、defer/决定身份和入口，不建计划执行框架或一组模型工具。验证：当前 P01—P24 证据矩阵已逐名对照 Pager、Shell、Control、Timeline、存储、import/rewind、PTY 测试；产品实现与未覆盖边界分别记录，不以 suite 总数替代场景。
- [x] 2.1 核定独立候选/不可变决定身份、Control architecture/schema 与消费者校验、defer EndTurn 收尾和 API；对齐在制恢复/存储准入接口，不新增兼容框架。
- [x] 2.2 统一 /view-plan 和 UI canonical artifact 有界异步读取，绑定 session/查看请求/版本和评论；复现并解决 legacy 路径差异。
- [x] 2.3 在同一 owner 持久保留一个候选，区分收起、延期、修订与放弃；覆盖提交失败/未知 ACK/提交后崩溃，不伪造审批或自动重弹。
- [x] 2.4 实现恢复审阅、跨 Behavior 保留及 fork/导入/rewind 的来源校验，不复制执行资格或抢占原 owner。

## 3. Approval and verification — completed

- [x] 3.1 批准绑定提交/版本/决定身份及不可变 action/feedback，接入既有 ResampleStep/PlanHandoff；核对重复/冲突、查询超前、候选替换与单次消费。
- [x] 3.2 保留现有 Drafting/Amending 修订语义、反馈一次消费和工具授权边界。
- [x] 3.3 实现接纳前保留候选/评论、artifact 与整 session GC 引用保护及顶部 task 布局下的入口；放弃不绕过剩余引用核对。
- [x] 3.4 执行 acceptance.md 的 P-01～P-24，包含原恢复审批 PTY 回归，记录真实运行结果。
- [x] 3.5 更新开发者说明，执行所需 Rust/格式检查及 OpenSpec 校验；未通过前不归档、不宣称已实现。


最终运行证据及分层验收范围见 [verification.md](verification.md) 文首；已完成归档；归档后全量与 archive 校验记录见版本联合验证。
