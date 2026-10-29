## Why
Goal pause/restart/definition/Behavior控制提交后、首个请求前仍可能沿用上次请求Goal scope。

## What Changes
在同一actor durable Control命令内随已确认的Control提交切换请求scope；失败写入保留旧投影，不增加第二份持久化权威或独立ACK。Shell共享Control提交入口计算现有Goal资格谓词，cold bring-up保留现有初始化。

## Impact
ChatState查询/投影、Shell控制入口与定向失败路径回归。
