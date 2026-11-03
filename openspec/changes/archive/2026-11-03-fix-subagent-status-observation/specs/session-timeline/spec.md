## ADDED Requirements

### Requirement: Model subagent queries retain access to authorized durable results

模型子任务状态查询在 coordinator 无该 child 注册时 SHALL 在原查询期限内只读核对当前来源的 durable Spawn、Ended 和适用的 child Result 链。lifecycle root 和 immediate parent SHALL 保留原有读取权限；无关来源、未知 ID 及隐藏的 Workflow child SHALL 保持授权拒绝且不泄漏存在性。历史查询 SHALL 使用有界只读 worker，不创建 writer、恢复执行或重新投递结果；有授权 Spawn 但无可信终态 SHALL 保持观察未知。

#### Scenario: Parent queries an original child after cold recovery
- **WHEN** 父会话恢复后 coordinator 没有原 child，但其 durable Spawn/Ended/Result 链可验证
- **THEN** 单项和多项查询返回原 completed、failed 或 cancelled 状态及适用结果，不把完成缓存缺项解释为权限丢失。

#### Scenario: Unrelated or hidden source is queried
- **WHEN** 来源未获 root/direct-parent 授权，或目标是 Workflow 隐藏 child 或未知 ID
- **THEN** 查询继续拒绝，不读取或泄漏目标结果。

#### Scenario: Historical result cannot be proven
- **WHEN** 来源已授权但无 terminal、Result 链损坏或只读查询超时
- **THEN** 返回对应观察未知或无效证据，不从文件改动、注册表缺项或部分正文生成任务终态。

### Requirement: Subagent observations remain independent of progress delivery

子 Agent 接入 coordinator 时 SHALL 使用已有真实 signals 初始化本地进度观察，普通启动与恢复原 Turn SHALL 使用相同的持续采样入口。UI 进度通知的交付确认 SHALL NOT 阻止后续本地观察更新；仍 SHALL 最多保留一项在途进度交付，并沿用既有取消与完成所有权。没有真实样本时 SHALL 保留观察未知，不虚构零计数。

#### Scenario: Parent queries a restored open child Turn
- **WHEN** child 从树挂起恢复原有未结束 Turn，父 Agent 查询该 child
- **THEN** 查询可取得真实 signals 的运行快照，不因没有提交新 prompt 而持续返回不可用。

#### Scenario: Progress gateway acknowledgment is blocked
- **WHEN** 一项 UI 进度通知等待网关确认且 child signals 继续变化
- **THEN** 本地查询反映后续样本，UI 通知仍只有原在途项；确认后后续通知使用当前样本。

### Requirement: Task output distinguishes observation errors from confirmed execution

get_task_output 的单项和多项结果 SHALL 将不可达、超时、无效证据分别表达为 observation_unavailable、observation_timed_out、observation_invalid_evidence，不将这些观察失败报告为 running 或任何任务终态。观察失败 SHALL 保留任务身份和未确认结果；授权拒绝继续独立处理。

#### Scenario: Subagent status cannot be observed
- **WHEN** 单项或多项 get_task_output 查询得到不可达、超时或无效证据
- **THEN** 结果返回对应观察失败状态和未知说明，既不确认运行，也不计入已完成数量。
