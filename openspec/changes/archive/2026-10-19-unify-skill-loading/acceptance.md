# 验收矩阵

本表定义验收要求，实际运行结果和覆盖限制见 [verification.md](verification.md)。静态审阅不计作产品验收通过，没有对应运行证据的场景仍未完成。各表中的 capability 链接指向本 change 的 delta；旧契约回归单独列出。

## 入口、身份与内容

| ID | 输入 / 故障注入 | 必须观察到的结果 | 契约 / 验证层 |
| --- | --- | --- | --- |
| SK-01 | 用户 `/skill args` 与模型 `load_skill` 请求同一来源 | 相同正文准备语义；无需前置任务模型决定是否加载，权限辅助调用仍走原管线；真实 trigger 不同；工具注册有效且无旧 Skill IO | [extension-runtime](specs/extension-runtime/spec.md)、[tool-authorization](specs/tool-authorization/spec.md)；注册 / Shell 定向测试 |
| SK-02 | user-only、model-only、禁用、未满足条件的技能；模型伪造用户来源 | 分别应用真实入口资格；模型不能获得 user-only 权限；不可用条目明确拒绝 | extension-runtime；目录 / 调度测试 |
| SK-03 | 重名、限定名、同路径原生与插件、选定来源移除 | 唯一身份可解析；插件正文与变量来自所选插件；歧义或来源消失失败且无 fallback | [configuration-rules](specs/configuration-rules/spec.md)；resolver / loader 测试 |
| SK-04 | 正文中的参数索引、缺失索引、无参数占位符；参数本身含 `$0` / `${SESSION_ID}` | 单次替换，不递归解释插入值；缺失索引为空；无占位符沿用参数附加语义 | 主规范 substitution 契约；内容单元测试 |
| SK-05 | Markdown 链接目的地址与标签/title 同名；元数据含引号、尖括号、`&` | 只解析链接目的地址；元数据正确编码、可还原；正文不被属性转义污染 | configuration-rules 及主规范 link 契约；格式化测试 |
| SK-06 | 空内存 body；合成 URI 缺 body；快照后磁盘变化；正文引用另一个 Skill | 空正文成功，缺正文失败；已有 body 不重读；引用资源不递归自动加载 | 主规范 snapshot 契约、design §3；内容测试 |
| SK-07 | 文件恰好 1 MiB、超过一个字节、非法 UTF-8 | 文件读取边界正常；超限 / 编码错误失败；读取不超过上限加探测字节；通过读取的正文仍须通过准备与请求预算 | 主规范 bounded body reads；文件测试 |
| SK-08 | 普通 `read_file(SKILL.md)` / skills 目录 Markdown；初始化 `skills:` | 文件保持 Read；初始化保留预加载角色和禁用规则；不产生用户加载行 | extension-runtime、[client-surfaces](specs/client-surfaces/spec.md)；工具 / 预加载 / 显示回归 |

## 输入、门控与结算

| ID | 输入 / 故障注入 | 必须观察到的结果 | 契约 / 验证层 |
| --- | --- | --- | --- |
| SK-09 | 单个用户技能输入，附带文本与附件 | 一次发布消费输入并加入原用户消息与独立结果；只显示一条原始输入；正文未拼进 user text，之后才开始任务采样 | [input-admission](specs/input-admission/spec.md)、[session-timeline](specs/session-timeline/spec.md)；端到端请求捕获 |
| SK-10 | 第一项准备成功、第二项失败、第三项尚未执行；第一项即失败 | 首个失败后停止派发；整组不发布且成功索引为空；区分失败、准备后取消与未执行，无部分 Loaded 或静默 None 继续 | input-admission、configuration-rules；Shell 测试 |
| SK-11 | 同名同参数和同名不同参数的重复引用 | 每引用独立 identity / 结果；展示索引的去重不合并执行 | input-admission、configuration-rules；resolver / 执行测试 |
| SK-12 | 内建命令、`/loop` 空引用分支、普通文本及现有 slash 边界样本 | 保持现有路由，只对解析出的技能引用派发 | input-admission；resolver 回归 |
| SK-13 | 等待队列、编辑 hold、保存编辑成功或失败、丢弃编辑、删除、消费竞争 | 等待不读正文；保存以新 identity 原位替换，失败保持原项和 hold；取消旧项不加载；不能越过 held head | input-admission 及主规范 queued editing；输入状态机测试 |
| SK-14 | 排队期间来源消失、禁用或失去资格 | 消费时失败，不能换成同名来源；未提交的本地编辑按最终提交解析 | input-admission、extension-runtime；目录变化测试 |
| SK-15 | 权限拒绝、PreToolUse deny、能力受限 child、allowed-tools 元数据 | 显式派发不绕过门控；拒绝前未读取的正文不读取；后续工具不获额外授权 | tool-authorization；门控测试 |
| SK-16 | Hook 拒绝、失败策略、取消、PostToolUse 观察及同输入重送 | 沿既有 Allow / Deny / Observe 和 on_failure 结算，Post Hook 晚于工具终态确认；已结算或结果不明的 Hook 不重跑；无新增改参 / 改正文接口 | tool-authorization、session-timeline；Hook 计数及顺序断言 |
| SK-17 | 前台用户加载失败，含 Active Goal 和后续排队输入 | 只结算所属失败输入，不采样它、不自动重试它；不永久占锁，后续按既有仲裁处理；失败原文不混入后续请求或恢复重建的模型 Surface | input-admission；Actor 状态机测试 |
| SK-18 | 运行中工具批次尚未完成时发技能插话；第二项准备失败 | 原批次闭合后使用 Runtime Step；插话及全部准备正文不进入 Surface，原任务继续；不制造 turn 归属 PluginUsed | input-admission、configuration-rules、[model-sampling](specs/model-sampling/spec.md)；批次 / interjection 测试 |
| SK-19 | 队列未消费取消、准备中取消、多技能发布前取消、发布与取消竞争 | 未消费不派发工具 Hook；发布前取消整组不入 Surface；发布已确认则保留 Loaded 历史，首次消费前终止按 SK-41 结算 | input-admission、client-surfaces、session-timeline；取消故障注入 |
| SK-20 | 模型自主加载失败后继续处理其工具结果 | 返回真实模型工具错误，不错误结算最初用户输入，也不由 Runtime 隐式 fallback 成文件读 | design §4、extension-runtime、tool-authorization；工具循环测试 |

## 上下文、持久化与恢复

| ID | 输入 / 故障注入 | 必须观察到的结果 | 契约 / 验证层 |
| --- | --- | --- | --- |
| SK-21 | 用户与模型分别成功加载，捕获 Timeline 和下一次请求 | 用户在真实 Runtime Step 内执行，结果关联该工具及输入组；模型结果关联真实模型 call；正文各贡献一次，无虚假 provider 请求、响应或用量 | session-timeline、model-sampling；持久事实 / 请求断言 |
| SK-22 | 正文准备后提交失败；提交成功但 ACK 丢失；UI 更新失败 | 确认前不 Loaded、不采样；未知先对账；已提交可重建 UI；插件诊断不把未知当确定成功或失败 | session-timeline、configuration-rules；存储故障注入 |
| SK-23 | 同 identity 同 payload 重送、冲突 payload、多客户端重送、新意图相同文字 | 原结果幂等返回；冲突拒绝；无重复 Hook/正文；新 identity 正常加载 | session-timeline、主规范 input retry；并发 / 恢复测试 |
| SK-24 | 文件修改或删除后 resume / export / import / fork / rewind | 读取原结果快照、无工具和 Hook 重跑；截断边界之外的事实不泄露到新上下文 | session-timeline；存储 / 历史操作回归 |
| SK-25 | 正文 artifact 缺失或损坏；历史只剩未确认加载 | 明确完整性或未确认状态；不借当前磁盘修补、不在 replay 中自动执行；执行 owner 对账 | session-timeline；损坏 fixture |
| SK-26 | Chat Completions、Responses、Messages 及 native continuation | 成对且唯一的协议身份；批次不被拆开；Runtime 投影不被再次调度；native 可安全追加或明确重建 / 失败 | model-sampling；各 backend 请求 fixture |
| SK-27 | 原用户输入很长；准备正文合格但完整请求超限；附件 / 工具定义占用额度 | 按当前 revision / model / route 复核全部请求及输出预留；准备后放不下明确失败，无静默截断；不能在 open tool Step 内启动压缩来绕过失败 | model-sampling；上下文预算 / 生命周期测试 |
| SK-28 | 首次关联模型步骤获得已接纳响应后 compaction，再次显式加载 | 已消费内容按普通历史压缩；新加载按新 identity 交付正文，稳定 System prefix 不变；首次响应前场景另见 SK-36 | model-sampling、[context-compaction](specs/context-compaction/spec.md)、client-surfaces；压缩回归 |
| SK-29 | 插件组部分准备后失败、整组成功、插话和发布 ACK 不明 | 未发布组的已派发项不报成功，未派发项不伪造使用；整组发布后各项成功；插话不新增 turn 归属；未知先对账 | configuration-rules；诊断事件断言 |

## 主窗口交互

| ID | 输入 / 故障注入 | 必须观察到的结果 | 契约 / 验证层 |
| --- | --- | --- | --- |
| SK-30 | 用户与 Agent 加载中的 normal 主窗口 | 相同 Skill 行交互；名称、真实来源、加载中可见，无需模型叙述确认 | client-surfaces；Pager snapshot / mock provider smoke |
| SK-31 | 完成、拒绝、组失败、取消、提交未知；多个 Skill 行折叠正文 | 更新原行；准备完成仍 Loading，显式 Skill 不参加匿名数量合并，名称与结果保留；未知不显示 Loaded，无重复成功行 | client-surfaces；状态更新 / 分组测试 |
| SK-32 | 展开已加载 Skill 后修改源文件 | 展开显示实际来源、调用参数和原提交快照，保持现有折叠与正文浏览交互 | client-surfaces；Pager snapshot |
| SK-33 | normal、minimal、resume replay、headless 对同一事实 | 身份、来源与结果一致；headless 结构化输出；不重执行、不产生重复行 | client-surfaces；跨 surface 集成测试 |
| SK-34 | queued steering 与无队列 identity 的直接插话，含图片和用户字面 `user_query` 标签 | 同样的加载前提和稳定关联身份；用户显示保留原文与图片，不从标签恢复显示；模型技能正文来自独立结果 | client-surfaces；interjection / replay 回归 |

## R1—R6 修订回归

| ID | 输入 / 故障注入 | 必须观察到的结果 | 契约 / 验证层 |
| --- | --- | --- | --- |
| SK-35 | 第一项正文分别使用参数内联与参数后缀，第二项失败或取消；随后原任务继续及新输入到来 | 失败组的原文、参数和正文均未进入后续请求；准备快照仅为历史，非 Loaded；没有隐式指令泄漏 | input-admission、session-timeline；请求捕获，关闭 R1 |
| SK-36 | 正文发布后、首次已接纳响应前触发 pre-prune、异步压缩发布、模型缩窗、图片投影与请求重试 | 用户 / 模型来源均保留全文至首次已接纳响应；用户项合法换 ID 后仍关联当前组，不因旧 ID 消失或请求已发出而解除保护；无法继续时按 SK-41 结算 | context-compaction、model-sampling；压缩 / 请求恢复 fixture，关闭 R2 |
| SK-37 | 首条输入无 active step，依次验证 allow / deny / cancel、工具终态与发布 ACK 丢失 | Runtime Step 与 ToolEvent / Hook cause 均通过真实 Timeline fold；FIFO Handled、steer Invalidated 合法；无虚假 provider usage，无提前 ACP Completed | session-timeline、tool-authorization、client-surfaces；生命周期 / 事件顺序测试，关闭 R3 |
| SK-38 | 选择器选中后、首次接纳前来源删除 / 禁用 / 同名替换；伪造显示标记 | 显式意图保留，来源失效明确失败，不退化普通 prompt、不换来源；伪造标记不授予权限；普通未知 slash 保持原路由 | input-admission；Pager → payload → Shell 测试，关闭 R4 |
| SK-39 | 重复占位符、小测试额度、单项与组总量边界、长度溢出及展开取消 | checked length / 有界构造在大分配前拒绝；实际正文单项和用户组额度均为 1 MiB；无部分 Loaded，取消能在工作片段边界结束 | configuration-rules；有界准备测试，关闭 R6；R5 由 SK-16 验证 |
| SK-40 | 原输入及多技能发布边界注入崩溃、ACK 丢失、UI 发送失败 | 只存在整组未发布或整组已发布；恢复无半条输入、重复正文或 Hook；UI 从权威结果重建 | session-timeline；原子发布 / 恢复 fixture，关闭 R1/R3 |
| SK-41 | 发布确认后、首次已接纳响应前取消、投影失败或请求终止，包含图片已合法换 ID 的用户项；随后 resume / fork / 新输入 | Loaded 历史保留并说明执行未完成；当前用户组完整撤下，模型结果换成同 call id 交付错误，无关内容保持；未确认结算时不采样；不延迟执行失败指令 | input-admission、model-sampling、session-timeline；终止 / 历史投影 fixture，关闭 R2 并复核 R1 |

## 与未实施 change 的集成条件

`suspend-session-tree` 也是提案，不能把它当作当前运行能力。如果它先实现，追加挂起时尚未派发 / Loading / 已提交 / ACK 不明四个 fixture，验证挂起不改成 Cancelled、恢复不重复执行已提交结果、旧 Hook 不重跑，并由其执行 owner 续接。本 change 先实现时，在后续挂起任务中纳入同样检查。这是实施顺序约束，不是本次新增挂起功能。

## 实施时的验证方法

优先为上述失败边界添加定向测试，复用现有工具、Shell、ChatState、sampling-types 与 Pager fixtures；不为文案或简单字段写只镜像实现的测试。具体测试名在实现后登记到 `verification.md`。

基础命令以 `docs/development.md` 为准：先运行受影响 package 的定向 `cargo test --locked`，再按实际影响运行必要的跨层回归；构建尽量串行，避免并发 Cargo 扩大磁盘使用。最后进行 mock provider 的 normal/minimal/headless smoke，并运行 `openspec validate --all --strict --no-interactive`。归档前逐行记录结果、失败与未覆盖项，不以文档校验替代上述运行验证。
