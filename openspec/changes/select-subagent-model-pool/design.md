# Design — 待用户同意

## 范围和用户体验

命令为 `/subagent-models`。面板沿现有 ModalWindow 布局，按 provider 分组、`/` 搜索、Space 勾选；底部显示 `Enter 应用 / Esc 取消`。修改先在面板草稿中积累，Enter 一次提交完整集合，避免多次点击在同一模型步骤中产生多个短暂权限版本。没有 effort 栏，也没有“全部自动放开”的隐含默认。当前主模型作为“始终可继承”的固定行显示，其他行是用户授予的额外选择。空集合显示“仅当前主模型；effort 由 Agent 自选”。

池作用于当前主会话及其委派树，在该会话重连、cold load、压缩后恢复，不成为其他会话的全局默认。普通 fork 复制创建时池为新会话独立状态，后续互不修改。这里选择会话范围，因为用户说“当前 Agent”，也只有 ask 偏好明确要求全局持久设置；若用户希望全局默认，应在同意方案时明确扩展，当前不预建两层配置。

`effective_models = {root 当前有效模型} ∪ (selected_models ∩ 当前可选 catalog)`。selected_models 保存 canonical provider/model ID，不保存 API alias 或密钥。catalog 的 hidden、disabled、管理 allowlist 和已有权限继续约束；用户勾选不能增加目录中本来不可选的模型。移除目录项时保留选择记录但显示“当前不可用”，模型侧只列 effective 集合；目录恢复后重新计算。主模型切换后固定继承项随之更新，原主模型只有曾被用户显式选入才继续作为额外候选。

## 状态归属与应用边界

SessionActor 拥有池。沿 `SessionControlSnapshot` 增加 `subagent_model_pool`，包含 selected IDs 和生效版本；通过已有 ControlEvent 写入，不另建配置文件或持久 registry。池版本引用最近一次改变本池状态的既有 `ControlEvent.revision`，不另造独立计数器；选中集合变化或目录/根模型变化导致有效集合变化时，都在原控制边界同步提交新版本。即使只修改当前不可用的选择记录，也必须改变版本，避免并发编辑静默覆盖。全量模型目录仍由 ModelsManager/PublishedModelCatalog 持有。读模型目录不能当作用户授权池。

修改请求走已有 desired-state control 形态，携带 session、client intent 和 based-on revision。空闲时立即应用；正在生成或工具批次中时显示待应用，等本次 Step 关闭后应用。应用前不暂停当前请求；失败不把草稿显示为已生效。旧版本 UI 推送不能覆盖已应用的新版本。两客户端同时改池时以 revision 检查显式拒绝过期写入并返回当前快照，避免静默抹掉另一客户端勾选。

一次成功提交同时包含：池快照、同 revision 的当前模型上下文、控制回执。确认持久后才交换 live policy 与工具描述，再发 UI applied。提交失败保留旧池、旧投影和旧权限，不存在“UI已更新但runtime仍用旧池”的成功回执。

## 主 Agent 如何感知

沿 Grow 已有模式做三件事，三者从同一快照派生：

1. **工具目录**：更新 `spawn_subagent` 和 `set_subagent_model` 的 model 指南，列出当前 effective canonical IDs 与每个模型支持的 effort。池内模型可由 Agent 自选，不再要求每次用户点名；省略 model 仍按现有继承规则。effort-only 控制不改变模型。
2. **模型上下文**：在生效的 Step 边界，以现有 ControlEvent 的模型上下文机制写一条明确的系统提醒，带 revision、当前完整可用集合、增删项及 effort 自选规则。“此前列表仅为历史，本版生效”需要明确。复用 ControlContext 管线，增加独立 `SubagentModelPool` layer，不能冒用 AgentRole 或把池变化写成用户消息。旧层在该边界退休，保证压缩和重放只恢复最新有效池。连续多次尚未应用的修改可按现有控制合并政策收敛，不能遗漏最终集合。
3. **真实准入**：工具模型 validator、Shell fresh/resume override 和 `set_subagent_model` 读取同 revision 的池快照；范围外返回带当前版本/可用集合的错误，不能只靠模型遵守提醒。模型的一个旧请求已在途时不会热改它的 schema 或响应；尚未获准的旧池调用按提交后的当前快照重新判定。

示例：Step 运行时用户把额外池从 `{provider/A}` 改为 `{provider/B}`。面板先显示“待应用”；该 Step 原已接纳的 A 任务继续。Step 结束后一次持久 Control 提交 revision 8，工具描述和下一模型请求前的系统提醒同时变为 `{当前主模型, provider/B}`。之后模型再申请 A，执行端明确拒绝并附 revision 8，而不是悄悄替换成 B。若主会话当前空闲，不额外触发一次付费模型请求；提醒在下一次正常推理前交付。

初次会话将池规则并入现有初始化上下文。cold load 和压缩从最新 Control 快照重建池与该层提醒，不依赖历史某条工具描述，也不重复注入全部旧变更。

## 子 Agent、resume 和 effort

池是根会话授予委派树的上限，子 Agent 不能用嵌套派发扩大。直接调派者仍须满足现有 Task 能力、深度、TODO 和权限准入。新 spawn 的 model 省略时先按现规则继承直接父模型，再检查它是否仍在当前根池内；若父模型已被移出池，要求显式选当前允许的模型，不静默改模型。显式 model 与 set_subagent_model 都受同一池约束。

已运行子任务移出池后可以按原模型完成，不中断已在途推理；新的模型切换和新的委派以当前池为准。对已运行、保留旧模型的子任务，effort-only 调整仍可用，因为没有产生另一模型授权。新恢复的历史任务若省略 model，沿现有 retained selection 恢复；如果该模型已不在当前池，则拒绝新的执行准入并要求用户重新加入或主 Agent 显式选允许模型，不能凭旧历史重新扩权。纯只读结果观察不受池影响。冷恢复已准入但中断的同一次 attempt 保留原不可变授权事实，和发起新 resume 分开判定。

effort 不存入池；可选值取自实际目标模型的 catalog。Agent 可以自由选择目标支持的 canonical effort，省略则沿现有工具默认。范围外 effort 在实际控制准入报错，不自动替换或回退。主模型相同不等于强制 effort 相同。

Workflow Run 的冻结模型路由和定义授权属于另一 owner，本轮不修改；面板说明影响普通工具委派。不能把 session 池反向写入既有 Run 并改变已冻结的 provider 证据。

## 取舍与验收

仅改系统提示最便宜，但无法阻止旧上下文或错误调用越过池，故不采用。仅放全局 config 会改变其他会话、且 file watcher 与会话生效顺序难以解释，本轮采用现有会话 Control。单独广播临时提醒会在断线/压缩后丢失，故用已有可恢复模型上下文层；新增 layer 是区分独立可退休契约所需，没有新增管理框架。

必须验收默认空池、自由effort、busy Step 中改池、两个客户端revision竞争、范围外直接spawn/resume/set、已运行任务保留、嵌套不扩权、主模型切换、目录失效、write/ACK失败、reconnect/cold/compaction。Tier3 用实际 Pager slash 操作→Shell Control→下一mock provider请求读取同版提醒和工具schema→真实工具派发验证，不能只证明 reminder helper 字符串正确。持久失败不得先回applied；旧请求晚回不允许覆盖当前revision。用户同意前停止在本设计与候选spec。
