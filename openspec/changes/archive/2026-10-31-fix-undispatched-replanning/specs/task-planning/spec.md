## ADDED Requirements

### Requirement: Dispatch failures close child replanning responsibilities

明确记录 dispatch_failure 的关联 SHALL NOT 被父项后续修改重新赋予 child 确认责任。来源更新 SHALL 保留为带原失败原因的 Unapplied，关联保持非 terminal 且不生成结果。既有未闭合 replan SHALL 可依据同一失败事实由父 owner 持久闭合；恢复投影前 SHALL 完成该闭合，重复恢复 SHALL 幂等。未派发事实不自动解除必需交付验收，Released 不单独证明活跃或未知执行已结束。

#### Scenario: Parent edits a step after child startup failure
- **WHEN** child 在启动准入前失败且已记录 dispatch_failure，随后父 TODO 的要求、优先级、非完成状态或移除发生变化
- **THEN** 来源变化记录为 Unapplied，pending_source_revision 保持为空，不等待无法运行的 child；必需交付仍须明确 Released。

#### Scenario: Resume a released dispatch failure with stale replans
- **WHEN** 恢复的计划包含 dispatch_failure、Released 和遗留 Pending 或 Dispatching replan
- **THEN** 父 owner 在投影前持久闭合这些 replan，保留来源、失败原因及非 terminal、无结果事实，完成检查和 scope 切换可以通过，重复恢复不再提交。

#### Scenario: Repeat the original failure acknowledgement
- **WHEN** 同一 dispatch_failure 再次提交，但关联上仍有遗留未处理来源
- **THEN** 闭合遗留来源；已经闭合时幂等，不改写不同的失败原因。

#### Scenario: Released live or unknown child retains its obligations
- **WHEN** 关联没有 terminal 或 dispatch_failure 证据，父级只提交 Released
- **THEN** 执行和重规划义务仍可见并阻止完成，不借恢复清理伪造终态。

### Requirement: Unchanged completion blockers stop automatic sampling

同一模型循环 SHALL 至多为完全相同的完成阻塞注入一次处理提示。下一次完成尝试若阻塞未改变，SHALL 返回包含具体阻塞的 planning_completion_blocked 错误并停止模型采样和 Agent completion recovery。该错误 SHALL NOT 宣告工作完成；Goal 内部回合 SHALL 沿现有错误路径暂停自动续跑。新用户回合可以再次处理原义务。

#### Scenario: Required obligation cannot be resolved by the current agent
- **WHEN** 首次完成检查提示待处理义务，代理检查或调用工具后，相同 blockers 在下一次 final 仍存在
- **THEN** 本轮停止并保留具体义务，不再以相同提示持续调用模型，即使没有配置 max-turns。

#### Scenario: Structured output encounters the same unresolved obligation
- **WHEN** StructuredOutput 的两次完成尝试之间 blockers 完全相同
- **THEN** 使用普通 final 的同一停止规则，不发布成功完成。

#### Scenario: Agent resolves the obligation after the reminder
- **WHEN** 提示之后代理合法解除阻塞，再次尝试完成
- **THEN** 正常完成；改变阻塞集合或合法依赖等待仍沿原处理流程推进。
