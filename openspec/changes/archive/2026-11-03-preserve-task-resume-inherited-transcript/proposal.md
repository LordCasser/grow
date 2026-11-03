## Why
Task resume_from派生新child继承模型Surface，却未为继承的消息保存presentation事实。Seed不是UI日志，冷重连只能看到新epoch正文。当前展示与模型上下文产生缺口。

## What Changes
从canonical source校验使用的同一validated cut取当前分支的完整消息/thinking/tool transcript；与新child Seed原子发布，匹配的正文引用新Seed，其余独立历史展示继续作为有限事实。源Control、runtime、子任务、路由、usage、Plan当前状态及交互不继承执行或UI当前态。普通fork/bootstrap不变。

## Impact
Shell Task resume source、child initial persistence和现有history projector；不增加第二日志、迁移或执行authority，不把Seed自动显示为用户历史。
