## Why

SAMPLER-1：同一公开RequestId替换登记后，旧task正常完成会删除新登记，panic不清理；两个collect入口的CancelOnDrop也按公开ID取消，可能误伤新提交。它们共用根因是把可复用请求名当作具体工作实例。

## What Changes

Sampler owner以具体任务身份确认完成/异常清理，并把collect future的Drop取消绑定到该次提交token；public cancel(id)/query仍针对当前登记。保留同ID替换会cooperatively取消旧工作、usage/evidence独立结算及现事件ID。复用Tokio JoinSet身份和CancellationToken，不新建公共代际协议。

## Capabilities

### Modified Capabilities

- model-sampling：同ID替换、完成/异常/Drop均只影响其任务实例。

## Impact

仅sampler actor/state/handle/internal commands与相关测试，公共API签名不变。高风险并发归属Tier3，公共Handle控制provider/settlement屏障和独立review；属于统一审计修复的精确身份主题，与MCP连接身份分开实现。
