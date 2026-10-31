# Agent 定义与授权审计

审计日期：2026-10-31。源码 HEAD：e4d82ebd6；本机安装版本：grow 2.5.4 (069f712)。下述引用均为项目相对路径。

## 真实事件

截图对应会话为 `01a18aab-acf5-7fb2-a681-38402e52f0cc`，工作目录为本机 `workspace/vulnerability/CVE-2026-98432`。本地该会话 `timeline.jsonl` 中：

- 第 51899 行 Control snapshot 已选择 `vulnerability-analyst/vulnerability-analyst`，第 51900 行展示投影一致。
- 第 52316 行记录 2026-10-31 09:06:24.990（Asia/Shanghai）`general-purpose` 调用失败，原因被写为 `disabled via [subagents.toggle] in config.toml`。
- 第 52353 行包含截图中的“报错但配置里是 true”原文。
- 第 52362 行记录 `vulnerability-analyst` 短名调用失败，原因相同。
- 第 52375 行包含截图中的“两种类型都被判为禁用”原文。

未复制会话正文、研究对象内容或完整个人配置。当前 `~/.grow/config.toml:23-29` 的全局 enabled、两个短名 toggle 都为 true，子 Agent permission_mode 为 always-approve。当前 `~/.grow/agents/vulnerability-analyst/vulnerability-analyst.md` 的 `subagents.allow` 只有 `reverse/reverse-engineering-expert` 和 `malware-analyst/malware-deep-analyst`。

## A1：父 Agent 拒绝被错误归因到全局 toggle

确定缺陷。`crates/codegen/shell/src/agent/subagent/mod.rs:1856` 的 `validate_subagent_type` 先执行 parent filter；不允许即返回无原因的 `Disabled`，不会读取该名称的 toggle，也不会验证名称是否存在。真正的 toggle=false 同样返回该值。

`crates/codegen/tools/src/implementations/grow_build/task/mod.rs:381` 将所有 `Disabled` 固定渲染为配置禁用。这使“全局 true + 父 Agent 不允许”必然产生截图中的误导错误。`always-approve` 属于工具请求审批层，不能覆盖父 Agent 的委派资格；该次校验函数也未读取 permission mode。

当前父定义不允许 general-purpose，因此被拒绝符合现有 filter 语义；错误在于向模型报告了另一个配置来源，并且没有给出当前可用的完整 ID。随后重试另一类型不能修复父策略拒绝。

现有 `crates/codegen/shell/src/agent/subagent/tests/mod.rs:3101` 恰好将 parent filter 拒绝断言为 `Disabled`；工具测试另行验证固定 toggle 文案。两侧测试各自通过也无法证明归因正确。

最小修复：在现有 validation outcome 中保留拒绝原因，区分父策略拒绝、全局禁用、未知名称和 owner 不可用；共用同一决策供预检、实际派发和 describe 使用。父策略拒绝显示父 Agent 身份及有效可用 ID，不新增一套权限系统。补跨层测试：toggle=true、allow 不含目标时不能声称 toggle=false。

## A2：路径 ID 与 frontmatter 短名造成配置错位，校验顺序掩盖未知名称

确定的本机定义/配置错位。`crates/codegen/agent/src/discovery.rs:634-696` 从相对 Markdown 路径生成 ID，并覆盖 frontmatter 的 `name`；按名查找在 `:593-600` 直接拼接 `<id>.md`。

因此此文件的实际 ID 是 `vulnerability-analyst/vulnerability-analyst`。当前 `[subagents.toggle].vulnerability-analyst=true` 不控制该文件，短名也不会因 frontmatter name 相同而成为别名。toggle 缺失默认 true，所以错误短名条目不是此定义被禁用的原因；同样，若将这个错误短名设为 false，也不能禁用完整 ID。

对当前受限父 Agent，两个尝试都先被 filter 拦截，短名的 Unknown 被掩盖。对于没有委派限制的父 Agent，该短名应进入发现失败路径，返回 Unknown 及 available。这是区分身份问题与授权问题的关键反例。

最小修复：诊断明确区分请求 ID 与真实定义 ID，目录和配置编辑使用现有 path ID；为短名/路径 ID 与 filter 的组合增加回归。不建议增加隐式 basename alias，它会引入同名冲突和授权歧义。个人 allowlist 是否允许 general-purpose 或自身是用户策略选择，不在分析任务中自动修改。

## A3：Agents 面板持久化 toggle 后，运行时仍使用启动值

确定的代码路径缺口，未证实为此次截图的触发原因。`crates/codegen/pager/src/views/agents_modal.rs:925-933` 的 t 键写入 config 并立即翻转 UI entry.enabled；`toggle_agent` 只持久化文件。

`crates/codegen/shell/src/agent/config.rs:1355-1361` 在启动 resolver 中把 toggle 存入 Config；`crates/codegen/shell/src/agent/mvp_agent/subagent_coordinator.rs:386-400` 校验时克隆 `cfg.subagent_toggle`。`crates/codegen/shell/src/config/reloader.rs:14-35,149-238` 没有 subagents 更新事件或该节的 diff；接收方也没有相应分支。

所以用户在运行中把 false 改成 true，可以看到 UI 和磁盘已经开启，但当前运行时仍拒绝；反向 true→false 也可能继续使用旧许可。仅修改文件不足以同步模型目录与派发校验。这与 A1 独立，不能通过改报错文案解决。

最小修复应独立建 change，明确 toggle 生效时点，通过既有配置更新入口同步 runtime、会话工具目录与执行校验。Settings 的 child permission mode 已明确标记重启生效，不将其顺带改成热更新，也不把运行期禁用扩成取消已运行任务。

## 契约与覆盖

`configuration-rules/spec.md` 的 Agent rule discovery 指 AGENTS.md/规则文件，不能代替 Agent definition 身份和委派契约。当前归档规范没有完整覆盖上述 ID、filter、toggle、目录与执行一致性；`tool-authorization/spec.md` 的 deny 优先和不可扩大的 delegated ceiling 仍是约束，不能用 always-approve 绕开。

Task builder 已在 `crates/codegen/agent/src/builder.rs:646-680` 按 filter 过滤主 Agent 的目录，未发现所有目录都无条件展示的证据。此次模型在报错后直接查看磁盘目录并猜短名，不能据此认定首次工具描述就包含错误类型。

后续先修 A1 并覆盖 A2 组合，再独立处理 A3；不顺带重构 capability、MCP 或权限审批框架。
