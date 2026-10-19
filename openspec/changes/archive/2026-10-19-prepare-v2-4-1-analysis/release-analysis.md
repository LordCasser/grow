# v2.4.1 发布前分析 — 2026-10-19

## 结论与版本范围

本轮实现可作为 v2.4.1 候选内容，但归档与本地回归不等于完成发布验收。正式发布前仍须准备版本和 release notes，并让最终发布 commit 的全部托管门禁通过。本次只归档、整理、合入本地 main 和分析，不改版本、不推送、不打 tag 或发布。

远程只读核对时，main 和 v2.4.0 annotated tag 均指向 `cd5d88ec7f5d5c42ee18277304c352232dcb4e14`；v2.4.1 tag 尚不存在。[v2.4.0 Release](https://github.com/LordCasser/grow/releases/tag/v2.4.0) 为 public、非 prerelease，已有十个平台归档及 SHA256SUMS，共 11 个资产。本地 main 已有 `f99310f7` 和 `4a560b63` 两项 Plan 提交；它们属于下一发布候选的范围，不能只检查本轮八项 change。上述远程状态仅为分析时快照。

## 候选改动与相互作用

| 范围 | 用户可见变化与整合边界 |
| --- | --- |
| Transcript / trajectory 导出 | 文件写入和 sync 保持严格；只在支持的 Unix 路径打开目录 sync，Windows 避开不支持的目录句柄操作。真实父子 ledger 经 HTTP 到 tar 解包已有测试，HTTP 取消与临时文件释放也覆盖。用户 2.3.1 的笼统错误文本不足以证明只有这一种故障，不能承诺涵盖权限、磁盘和任意第三方代理问题。 |
| Skill | 沿启动/刷新扫描建立的目录按名称加载；目录只给元数据，正文及单个资源按需读取。禁用、不可用和拒绝明确报错，不指导 Read/Shell/另一副本绕过。普通文件审阅保持 Read，加载必须经 LoadSkill 授权和 durable 发布。新 resource 参数同时修正 Timeline 校验；资源不递归加载、不执行脚本。 |
| 视觉与通信展示 | 视觉 Sideband 只含图片及固定英文 prompt；来源引用只作本地证据。视觉进度在一个英文操作行内更新，按 prompt 身份结算，兼顾重连、离线截点及迟到终态。入站 Agent 消息使用 durable receipt 去重，Received 只代表接收，不能暗示执行或安全写入交接。 |
| 控制与 Plan | 重选相同 Agent/model/effort 保留控制回执，隐藏虚假 Switching 提示；配置或 effort 实际改变仍显示。main 的 Agent 替换同时重绑原 TODO owner，Plan 完成报告只读保留；静默的 Agent/Sampling no-op 不改变手动 Behavior 选择清除已完成 Plan 的规则。 |
| 采样失败、子预算和 Goal | 三次完整候选失败结束当前逻辑请求，后续用户输入经真实终态收尾建立新 Turn。精确已结算子预算不因失败被误耗尽；未知/未确认终态继续关闭准入。普通继续可在停止的 Goal 外交互，不复活 Goal、不重置预算；自动继续仍需显式 restart 并通过原预算门控。main 的 Plan 完成恢复采样不改这个 Goal 边界。 |
| 文件写入指引 | 共享 Mandatory Core 默认将写入放在项目/worktree、临时文件放系统临时目录；外部写入需用户目标或任务必要性。是新构建提示词的指导，不是运行时 sandbox，也不重写旧 session 已持久化的 system head。 |
| 发布门禁 | core、原生协调/导出、Windows storage 和 OpenSpec 接收并验证 tag 对应的同一完整 SHA。任一失败、跳过或取消均阻止 publish。精确测试先列举唯一名称再验证实际执行一次，避免旧名称产生 0 test 假绿。 |

八项实现的验收与限制保存在各自归档 verification；当前契约已合入 `openspec/specs/`，现行说明不再依赖活动 delta。组合回归另发现新增写入位置指引漏跑了已有 prompt-size 测试：三项断言失败。指引从 550 bytes 精简为 373 bytes，保持全部语义；为明确新增的共享安全规则给原上限各增加 512 bytes。最终渲染为 8495/8020 bytes，继续受完整尺寸及只读相对尺寸断言约束。此修正不更改角色、装配或授权契约，验证结果见 verification。

## 发布前必须完成

1. **版本与发布材料**：`Cargo.toml` workspace 仍为 2.4.0；57 个本地包与 Cargo.lock 一致，其中 33 个继承产品版本，24 个独立版本不应强行改号。正式准备版本时只更新继承链和相应 lock 条目，新增 `crates/codegen/shell/changelogs/2.4.1.md`，包含本轮八项及上述两项 Plan 修复，明确 MCP 多轮仍暂缓。不可改写 2.4.0 历史说明来充当新版本说明。
2. **确定发布源码**：MCP 工作区仍有未提交改动，不能以这个 dirty tree 构建、打 tag 或声称验收。使用已提交的候选或隔离干净 checkout；版本/说明完成后确定最终 commit，再推送 main 与对应 annotated tag。本轮没有把未提交 MCP 纳入候选。
3. **同 SHA 托管验收**：core 的真实 CLI/PTY 与协议恢复、macOS/Linux/Windows 原生协调和生产导出、Windows 长路径及 storage、全部规范/归档校验都须在最终 tag SHA 成功。已有本地 macOS 结果不可替代 Windows/Linux gate。
4. **产物验收**：十个平台 release-dist 构建成功；对可运行平台执行实际二进制 smoke，检查 macOS 签名、Windows grow.exe、每个 tar 单一入口、全部远端 attestation、SHA256SUMS 覆盖与下载重验。现有 workflow 在 draft 完成这些检查后自动转 public；本次未执行该流程。

## 剩余覆盖边界与风险分级

| 优先级 | 证据与边界 | 建议 |
| --- | --- | --- |
| 发布门槛 | 原生 Windows/Linux 尚未在本次最终源码执行；Windows 报告直接涉及平台文件系统。 | 任何对应 gate 未绿都停止发布，不用 macOS 的 cfg 检查代替。 |
| P2，独立债务 | `shell/src/session/actor/turn/mod.rs` 丢弃通用 `execute_tool_calls_result` 的 Err，可能让后续 Step 错误遮蔽原始错误；已登记 backlog。此问题属于工具/持久化失败收尾，与本轮 provider generation 恢复不同。 | 另建最小 change 覆盖 admission/result publication/terminal ACK 注入、无下一模型请求、冷恢复及原始错误保留；不在归档维护中改写状态机。若候选验收实际触发此错误则阻断发布。 |
| P2，独立核对 | main 的 backlog 已记录同步 compaction 清空前端 TODO，可能与 Timeline 计划再投影时序不一致。 | 保留独立调查；不把 Agent 切换 TODO 修复当成该路径的证据。 |
| P2，平台覆盖 | Windows storage/HTTP 导出 gate 运行 windows-2025 x86_64；windows-11-arm 发布构建运行进程协调 smoke，没有独立的导出用例。macOS/Linux 原生 gate 也不代表全部资产架构。 | Windows ARM64 上实际下载父子 transcript 可作为补充验收；若以后承诺全架构行为门禁，另立小 change 扩展所需矩阵。 |
| P3，发布耗时 | release validate 只检查 tag 与版本；notes 非空检查到 publish 才执行。本地 `validate-release.sh` 提前检查 notes，但需要操作者实际执行。 | 打 annotated tag 后先在干净候选运行该预检，避免十个平台构建结束才发现说明缺失；后续可独立前移低成本检查。 |
| P2，配置覆盖 | core 使用默认特性 debug 二进制；release-dist 将开启 Pager 中远端 leader 设置分支，并使用不同优化/终止策略。发布产物运行协调 smoke，但未复跑完整采样/Skill/Plan 场景。 | 现有本轮改动没有新增 release-dist 分支；保留构建与原生产 smoke。后续按风险选择关键场景在分发二进制上执行，不据默认特性测试宣称生产配置全覆盖。 |
| 已知运行限制 | riscv64、两个 musl 资产不在 build-one 上原生运行 smoke；OHOS 走专用容器构建/检查，`smoke: false` 不等于终端实机验证。 | 准确保留支持/验证边界，不把“十个包都构建”写成“十个平台行为已验收”。 |
| 证据范围 | 预算子 fixture 使用真实 run loop/sampler/ChatState 与受控 ACK，未启动完整父 Task coordinator 或冷恢复该预算子。 | 保留已确认的接线与多层独立证据，不宣称完整预算子树恢复已测。 |
| 模型行为 | 写入位置与禁止技能绕过的提示词已确定性交付，但不能保证任意模型严格遵从。既有 system head 不被版本升级重写。 | 发布说明区分指引、工具资格检查和 OS 隔离，不能承诺旧 session 自动获得新 system prompt。 |

MCP 多轮、Remote Control / Agent Host、Provider 缓存和 Recap 的真实路由优化继续保持原来的暂缓或待证据状态。本轮不为发布分析扩展这些方向。

## 建议顺序

先完成本地提交和组合验证；随后单独准备 2.4.1 版本及准确说明，在干净候选上确定 SHA，再执行现有发布流程。先前 latest-main、旧版本资产或本轮归档成功，都不能替代最终发布 SHA 的门禁结果。没有新失败证据时，不为追求形式上的穷举无限扩展测试矩阵。
