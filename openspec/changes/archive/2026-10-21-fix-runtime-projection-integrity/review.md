# 对抗性审查

独立 Luna reviewer 只读审查归整、provider 编码、恢复状态门与 Timeline proof。主任务执行测试。

- 真实结果：缺失与重复结果仍走既有配对规则，匹配 Model 元数据仅调整投影顺序。User、User SkillLoad、错误 call_id 不获透明语义。
- 持久化：原事件前缀不变；归整追加 IntegrityRepair；二次请求不追加修复；冷恢复保留真实成功正文。旧 pairing 变换生成的已提交修复仍可完整重放，伪造 payload 替换拒绝。
- Reviewer 提出“同一结果后的多条重复 Model 元数据”可能截断批次。复核 valid_model_skill_load 拒绝已有相同 call_id 的结果或元数据；新增 actor 测试确认重复发布失败且事件数量不变。该未准入输入不是正常生产状态，reviewer 撤回风险结论。多条不同 call_id 的合法技能元数据由集成测试覆盖顺序和幂等性。
- Responses：恶意文本、来源和 reply 字段原样保留在 JSON，外层声明不能当作人工授权；不生成模拟调用/思考。canonical 权限未修改。
- Chat：未学习或另一 backend 不补字段；当前 Chat 学习后覆盖 portable/live，保留原 reasoning；runtime-only 能确认一次恢复，重复拒绝不反复启用，真实换路由才重置。

复核最终结论：限定在本次三项和合法准入状态内，没有剩余确定问题。native 分段中插入 AgentMessage 的生产可达性另列 backlog，不混入本次修复。
