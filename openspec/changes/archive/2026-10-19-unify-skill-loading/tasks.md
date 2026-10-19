## 1. 本次立项交付

- [x] 1.1 核对当前管线与主规范，完成 proposal、design、delta 和 acceptance 初稿；验证：初稿七个 capability、SK-01—SK-34 覆盖所列范围，来源事实与未来要求分开。第二轮审查后的修订由 1.4 单独记录。
- [x] 1.2 完成静态对抗性审阅和 OpenSpec 校验，记录 verification；验证：修改契约不丢失旧场景，文档链接有效，单 change 与 `openspec validate --all --strict --no-interactive` 均通过，产品代码和主规范无本次修改。
- [x] 1.3 完成第二轮源码对照与对抗性审查，交付 audit；验证：六项发现均有设计位置、代码证据、修订建议和验收补充，未修改产品代码。
- [x] 1.4 处理 audit.md 的 R1—R6，修订设计、八个 capability 的 delta、实施任务及 SK-01—SK-41，并重新审阅；验证：每项有设计层关闭记录，静态核对与全量严格校验通过；运行验证仍待实施。

## 2. 统一加载与持久事实（实现与验证完成）

- [x] 2.1 在现有 SkillManager 上接入统一加载操作与调用资格校验，复用正文读取、快照、链接和单次替换，增加准备阶段的有界构造；验证：SK-02—SK-07、SK-39，覆盖来源、空正文、编码、文件上限、单项及用户组 1 MiB 额度、长度溢出和及时取消。
- [x] 2.2 增加必要的 Runtime 上下文项及来源 / 快照关联，扩展现有输入消费提交以原子发布用户输入与整组结果，模型路径复用 conditional tool-result commit；验证：SK-17、SK-21—SK-25、SK-40，ACK 不明不放行、重送不重执行，不建立第二套加载生命周期。
- [x] 2.3 完成各 Provider 的合法投影、完整请求预算复核及首次已接纳响应前的全文保护；使用现有 Surface replacement 结算未消费的终止内容；验证：SK-26—SK-28、SK-36、SK-41，无孤立结果、裁剪交付或失败指令残留，重试 / 模型切换 / 压缩均遵守该边界。
- [x] 2.4 注册模型可调用的 load_skill 并更新技能目录提示，接入现有能力、权限、Allow / Deny / Observe Hook 和取消门控；验证：SK-01、SK-15、SK-16、SK-20、SK-37，保持 Hook cause 与终态顺序，不新增参数或结果改写协议。

## 3. 用户输入与生命周期（实现与验证完成）

- [x] 3.1 普通 / 队列输入直接派发统一 loader，选择器保留显式意图与来源，接纳后冻结引用计划；复用真实 Runtime Step / ToolEvent，逐项准备后统一发布；验证：SK-09—SK-14、SK-17、SK-35、SK-37、SK-38，无前置任务模型决策、无 user-body 拼接，失败 FIFO 合法 Handled，队列编辑换新 identity。
- [x] 3.2 queued steering 与直接插话在闭合原工具批次后接入同类 Runtime Step，保留原文、图片、输入 Hook 及稳定关联身份；验证：SK-18、SK-34、SK-35、SK-37，失败只结算插话，参数内联 / 后缀均不泄漏。
- [x] 3.3 接通取消、插件诊断和恢复所有权，处理准备组失败、发布竞争、首次消费前终止及未知状态；验证：SK-19、SK-22—SK-25、SK-29、SK-40、SK-41。若 suspend-session-tree 已落地，同时执行 acceptance 中四种挂起状态的集成检查。

## 4. 复用 Skill 显示（实现与验证完成）

- [x] 4.1 复用 ACP ToolCall / ToolCallUpdate 和 Skill 行交互，以明确身份与权威发布结果更新；显式 Skill 行退出匿名数量合并，单行正文仍可折叠；验证：SK-08、SK-30—SK-32、SK-37，准备完成保持 Loading，发布确认后才 Loaded，普通文件读取显示 Read，无新增面板或通用分组器改造。
- [x] 4.2 从相同事实生成 normal、minimal、headless 和 replay 更新，避免重复行及磁盘重读；验证：SK-24、SK-28、SK-33、SK-34、SK-40、SK-41，首次消费前终止保留 Loaded 历史并显示后续执行错误，不冒充当前持续生效。

## 5. 验证与收尾（实施代码已获授权；仅在对应验收通过后勾选）

- [x] 5.1 完成尚缺的 SK-01—SK-41 运行验证，登记实际测试名、命令、结果与覆盖限制；验证：受影响 package 定向测试、必要跨层回归和 mock provider smoke 通过，已覆盖场景按具名证据核销，失败及未执行项不能计为通过。
- [x] 5.2 同步更新相关开发者说明和过时管线注释，清理已被统一入口替代的运行时展开调用方，保留预加载；验证：代码搜索及文档核对不存在新旧加载双入口或第二份需求权威。当前 `load_skill_with_body` 的 Shell 调用仅用于 Agent rebuild / Workflow frozen skill 预加载；模型与用户显式加载共用注册的 `load_skill`。`docs/development.md`、`docs/architecture/agent-core-timeline.md` 链接本 change 作为未归档契约来源。
- [x] 5.3 实现完成后核对 delta 场景与运行证据，完成全量严格校验再按仓库流程归档；验证：归档前后 `openspec validate --all --strict --no-interactive` 和归档校验通过。

最终运行证据及分层验收范围见 [verification.md](verification.md) 文首；已完成归档；归档后全量与 archive 校验记录见版本联合验证。
