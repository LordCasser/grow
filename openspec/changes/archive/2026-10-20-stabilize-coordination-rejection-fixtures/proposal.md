## Why
v2.4.3 首轮正式 run 39422108840 的 Windows native 门禁中，两项协调测试在既有 5 秒 deadline 内未结束。它们使用 create_test_actor 的 http://localhost 默认 provider，依赖未监听端口及时失败；这不是被测 sideband/单次审批语义所需的确定性前提。

## What Changes
两项用例复用现有 MockInferenceServer，指定本地随机端口的 HTTP 拒绝，固定观测一个真实 provider 请求。保留五秒 deadline、失败结算、sideband 不修改原 Surface、单次 attempt 及只提供一次性审批的所有断言。

## Boundary
仅测试夹具维护，skip_specs；不改变生产协调、权限、超时、重试或持久化行为。首轮未发布候选取消，修复后同一发布流程重验，不跳过失败门禁。
