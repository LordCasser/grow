## MODIFIED Requirements

### Requirement: Primary guidance advances independently ready work

Grow SHALL 指导 primary 在任务启动、结果返回、出现阻塞或关键前提改变后的自然检查点重新选择工作，优先处理交付瓶颈和能解锁后续工作的成果。等待 SHALL 基于实际依赖和现有工具语义；指引不得要求忙轮询、重复已委派工作或通过扩大范围保持忙碌。独立工作结束且下一步依赖已派发子任务时，指引 SHALL 明确使用专用 wait_subagent 对原 subagent ID 发起有界阻塞等待，不通过另起 sleep 命令、轮询循环或额外任务制造等待。等待返回 SHALL NOT 被指引视为子任务完成或成果验收。

#### Scenario: One result arrives before an unrelated task
- **WHEN** A 的结果已返回且可以解锁下一项工作，B 仍运行但与该工作无关
- **THEN** 指引要求及时检查 A 并推进被它解锁的工作，不默认等待 B。

#### Scenario: Useful independent work remains
- **WHEN** child 正在后台执行，parent 还有不重复且不依赖该 child 结果的有价值工作
- **THEN** 指引要求 parent 继续推进该工作，并在自然检查点处理结果。

#### Scenario: No useful independent work remains
- **WHEN** 下一步所需结果尚未到达且没有值得执行的独立工作
- **THEN** 指引要求通过专用 wait_subagent 对所需的原 subagent ID 发起有界阻塞等待，不另起 sleep 命令、轮询循环或额外任务，也不反复请求无变化快照。

#### Scenario: Multiple task wait is selected
- **WHEN** 工具说明介绍多 subagent ID 配合等待预算
- **THEN** 保持 wait-all 说明，并指导仅在下一步需要全部结果时使用该方式，不把它描述成 wait-any。

#### Scenario: Wait returns without a successful child result
- **WHEN** 等待到期、被用户消息打断，或返回失败、取消、仍运行或观察未知
- **THEN** 指引要求按真实状态决定下一步，不将本次等待结束等同于 child 完成或结果已验收，不另起占位任务替代等待。

证据与实施入口：`crates/codegen/agent/prompts/audience/primary.md`；`crates/common/tool-types/src/task.rs::{build_task_description,build_task_output_description}`；既有运行时 `crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs::wait_all_event_driven` 不修改。模型遵循效果需实际模型场景验证，静态文案核对不构成遵循保证。
