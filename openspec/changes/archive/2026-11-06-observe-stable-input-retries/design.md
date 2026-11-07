# Design: observe-stable-input-retries

## 决策依据

父架构约定见 `openspec/changes/complete-nondeferred-backlog/design.md` 末段与
`expose-canonical-input-lifecycle` 提供方接口。canonical 输入事实的唯一所有者
是 Timeline；Shell 接纳只读消费，不建第二登记表或平行通知管理器。

## 接口与流程

1. `run_human_input_admission_hook` 在运行 Hook 前，用
   `input_id_for_prompt(session_id, prompt_id)` 调
   `ChatStateHandle::input_state`（提供方已实现，直接 Timeline fold）。
   - `Err` → 接线错误，接纳失败。
   - `Some(lifecycle)` → 只读 `lifecycle.payload_ref` 对应 blob，用
     `comparable_input_payload`（排除冻结 skill_invocations）核对原完整
     payload/requested_route；intent 兼容规则：FIFO payload 的
     Prompt/Followup 互认，Steer 需精确匹配。不符 → 身份冲突错误。命中 →
     返回 `InputAdmissionAttempt::Existing`（携带 canonical
     `chat_state::InputState`）。
   - `None` → 全新接纳，走原 Hook/提交路径。
   - 删除旧 `find_existing_prompt_input` 事件 blob 遍历扫描；
     `submit_input_durably` `!created` 回退分支同样改用 canonical fold。
2. `admit_human_input` 对 Existing 各状态统一返回
   `AdmittedInput { input_id, replayed: true, replay_state: Some(state) }`：
   重试是观察，不再把 blocked/consumed 等折叠为错误字符串。
   `admit_human_steer` 保持现有应答形状，仅改为消费 canonical
   `InputState`（Allowed → Ok，Blocked → 原因错误，终态 → 终态错误）。
3. `PromptCompletionKind::AdmissionReplay { input_id, state }`（state 为
   `chat_state::InputState`）。`queue_input` 重试分支先发该完成种类（EndTurn、
   0 token、无 usage/structured output），再回原 `persist_ack`；替代旧
   Allowed 重试的假 `RemovedFromQueue`。queue 编辑保存路径对 replay 不重插、
   不把 blocked 重试当 fresh 失败。
4. ACP `MvpAgent::prompt`：为本请求创建 `persist_ack` oneshot；发送
   `QueuePrompt` 后用 biased `select!`（应答 rx 优先，其次 ack）。rx 先就绪
   且为 AdmissionReplay → 直接返回观察应答（meta 仅 sessionId、promptId、
   `grow/inputAdmission`），不发 Working/Idle、无 usage；ack 就绪才发布
   Working，之后正常等待完成。未取得 ack 的接纳错误不发布 end_activity
   Idle；执行后错误沿原完成/error 路径。

## 测试

真实入口（actor 命令/`queue_input`、ACP prompt 等价公共 API + 隔离 fixture）：
idle 接纳后 running 重试同 id/状态与反向对照；replacement 后旧 id
RouteSuperseded、Consumed/Handled/Blocked/Submitted 观察不变；ACK 丢失冷重试
Hook+输入仅一次；payload/显式 Steer 变化冲突；无新 TurnCompleted/假
rosterIdle/新 billing。既有长输入/图片/frozenSkill 断言保留。实际统一actor96项和CLI ACP验证见verification.md；最终测试不依赖真实用户配置或模型服务。
