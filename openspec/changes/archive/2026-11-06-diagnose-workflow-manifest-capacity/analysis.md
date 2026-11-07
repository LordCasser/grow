# Analysis: 25 × `Workflow manifest exceeds 524288 bytes`

## 结论（TL;DR，修正版）

**已确认**：`session::workflow::` 的 25 个失败统一来自 `store.rs:557-560` 对整个
manifest JSON 的 524288 字节硬检查；且 `WorkflowAgentCatalogSource::for_test`
使用 `SkillsConfig::default()` 时 skill discovery 会读取**本机真实 `~/.grow`
用户技能目录**并冻结全部 body 进 manifest —— 测试输入非 hermetic 是事实。

**尚未证明**：导致越界的各 snapshot（agent definitions 的 tool_config /
prompt_snapshot、frozen skill body、route/model catalog 等）的具体字节构成，
以及哪个成分是主导性可变输入。`~/.grow/skills` SKILL.md 原始合计 269,403
bytes < 512 KiB，本身不足以证明它是 25 fail 的直接成因。control已经确认整组失败依赖发现环境；各snapshot的字节组成仍未知，不能断言技能是唯一或主导成分。

## 已执行的隔离对照

父复制当时已编译的Shell测试产物到自有临时目录，固定SHA256 `d7c6261a6b5ed1ce1c4d6deed7018a78bfc7d068187a6f41fb140d5a5a6d03e5`，仅为测试子进程设置临时HOME/GROW_HOME，父环境与真实用户目录未变。相同 `session::workflow:: --test-threads=1 --nocapture` 组合在隔离root中 **147 passed / 0 failed，exit0**；原非隔离组合为25 failed。日志 `/tmp/grow-backlog-workflow-isolated-control.log` 与 `/tmp/grow-backlog-workflow-integrated.log`，执行task `01a1af93-67e4-7db2-96a2-eab48b4bf599` 保存产物hash和真实exit。

该结果证明这25项失败依赖发现环境输入，未证明只有skill body是哪一部分贡献，也没有测量manifest各字段精确字节。变化包含user skills/agents/config，不能将其缩成只有技能。当前无需提高512KiB上限或压缩/分片产品快照；本轮integration采用隔离环境验证。测试fixture依赖主机发现输入属于独立工具维护债务，未混入F4的行为修复。Control B不是解释本次失败所必需，仅在真实内置资源配置越过现上限的可观察场景出现后考虑。

## 已核实事实链

1. **失败检查点**：`store.rs:17` `MAX_WORKFLOW_MANIFEST_BYTES = 524288`
   （= `chat_state::MAX_WORKFLOW_INITIAL_MANIFEST_BYTES`）；`store.rs:557-560`
   序列化整包超限即 `Store("Workflow manifest exceeds 524288 bytes")`。
2. **manifest 范围**：`WorkflowRunManifest` 仅 `{version, state, script_revision}`
   （store.rs:24-28）；script/args 经 `register` 走独立文件。即越线对象是
   `WorkflowRunState`（tracker.rs:385-396）：`agent_definitions` snapshot +
   `skill_catalog`（`SkillInfo` 含 `body: Option<String>`，types.rs:114-116，
   capture 后全为 Some，必然序列化）+ route/model/catalog 快照。
3. **agent definitions 并非为空**（修正此前错误推断）：`capture_agent_definitions`
   （shell/src/agent/subagent/resolution/definition.rs:89-124）显式
   `names.extend(all_builtin_agent_definitions())`，并通过 `available_agent_names`
   / `all_subagents` 发现 user/project 子代理，filter 之后仍保留通过项。
   `for_test` 的空 `cli_agents: Vec::new()` 只表示无 CLI 覆盖源，**不**证明
   definitions map 为空。每个 `WorkflowAgentDefinitionSnapshot` 携带
   tool_config 与 prompt snapshot，可能是主要字节来源之一 —— 未实测，不下结论。
4. **skill discovery 非隔离**：`for_test` 传 `SkillsConfig::default()`
   （ignore/disabled/roots 空）；`user_skill_roots()` = `home/.grow`
   （agent/src/resource_roots.rs:8-10）。cwd 是 `std::env::temp_dir()`，
   Local/Repo scope 恒空；本机 `GROW_ROOT`/`GROW_USER`/`GROW_HOME` 均为空，
   `~/.grow/bundled`、`~/.grow/plugins`、`~/.grow/SKILL.md` 不存在。
   ⇒ capture 实际冻结 `~/.grow/skills` 下 21 个本机用户技能的全部 body。
5. **静态量级（非最终序列化字节）**：`find ~/.grow/skills -name 'SKILL.md'
   | xargs cat | wc -c` = **269,403 bytes**（最大 diagram-design 37,571 /
   design-taste-frontend 29,312 / iot-firmware-vuln-hunting 22,718）。
   该值仅是候选输入之一的原始文件量级，含 frontmatter；最终 JSON 需
   实测，不作为归因依据。

## production 边界 vs 测试环境（区分表述）

- **Production**：manifest 整包（state 含全部 frozen skill body + agent
  definitions snapshot + route/model catalog）受 512 KiB 硬上限，逐字节检查。
  这是**现有硬上限**，不是已确认的新 bug；是否调限/压缩属产品决策，
  当前无证据，不在本 change 提出实施方案。
- **测试环境**：确认非 hermetic —— discovery 读取真实 `~/.grow`，任何用户
  技能和用户Agent变化会改变manifest输入；已执行control证实隔离发现环境后全部147项通过，排除了该固定产物在空用户环境中仍普遍超限的假设。

## 未核实项 / 覆盖限制

- 无真实序列化 `json.len()`；isolated control的pass/fail证据见上，不作字节构成推断。
- 各 snapshot 构成（definitions tool_config/prompt_snapshot vs skill body vs
  route/model catalog）的字节占比未知。
- 25 个失败 test 各自 script/args 走独立文件、不进 manifest，故不参与越线
  （修正此前错误表述）；其内容差异不解释 fail/pass 分布。

## 后续验证边界

隔离对照已完成，不能再把它列成未执行。`user_skill_roots()`只从`dirs::home_dir()`建立用户root，不受GROW_HOME控制；因此父通过测试子进程的独立环境字典设置HOME/GROW_HOME，未改写父shell变量或全局配置。精确manifest各字段字节占比未测，不影响25项失败依赖发现环境的结论。真实内置资源容量或大用户资源场景若另有证据，再进行Control B，不为本轮归因预建容量方案。
