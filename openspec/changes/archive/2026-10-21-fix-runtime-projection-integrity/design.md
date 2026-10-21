## Invariants

工具真实成功结果必须保留；Model SkillLoad 是对应 ToolResult 的不可变附属事实，不是第二份执行或人工边界。普通 User/System/Assistant 仍关闭结果批次；仅与紧邻前一结果 call_id 匹配的 Model SkillLoad 可透明归整，User SkillLoad 不获此语义。

## Decisions

1. 在 sampling-types 提供小型纯函数，按 assistant 后的 ToolResult / 匹配 Model SkillLoad run 稳定归整为结果在前、元数据在后。当前 repair 在原配对变换前归整并记录变更计数，已有原始配对证明不变；当前 IntegrityRepair 仍以精确变换校验。ChatState 初始上下文亦先归整。
2. Portable 投影复用归整；prefix closure 跨合法模型技能元数据而不吞入 native span。实际 native fragment 不重排、不改签名；Messages 跳过 Model 元数据时不提前 flush 结果。
3. Responses AgentMessage 使用 user 角色承载明确标记的 Runtime agent 上下文，说明不是直接用户请求或授权，随后附既有完整 JSON；不新增 permission evidence，不改 receipt/reply/canonical identity。
4. Chat learned 状态必须覆盖 portable 与 live 的 synthetic assistant。只补空字符串，不借用此前 unrelated reasoning。ChatState enable 检查两种会产生 synthetic assistant 的事实及 native 缺字段情况；保持一次性、backend一致、route重置和共享预算。

## Verification

先保存失败回归，再核对真实结果、缺失结果、重复结果、用户边界、错误来源、portable prefix 各切点、native 三协议、恢复幂等和真实 Timeline 重放。审查状态转换与历史 proof 的边界；不以在线 provider 成功替代本地不变量，也不发送真实用户历史。
