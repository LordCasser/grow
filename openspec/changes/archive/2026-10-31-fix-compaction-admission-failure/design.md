## Context

普通 turn 在 Step 边界关闭上一 Step、应用控制、执行同步/异步压缩，再建立下一 Step；最终 provider gate 同时检查前台所有权、active Step 和 storage admission。现有布尔合并丢失存储拒绝的具体原因。压缩 token 只属于 generation，正常控制取消由原 foreground owner 结算。

## Goals / Non-Goals

目标是复现并修复已确认的错误终态，主动验证 generation 取消竞态。非目标是以合成测试宣称命中了无现场截图、放开已关闭准入或增加自动重试。

## Decisions

保留原 admission 锁顺序和控制优先级；当前台及 active Step 检查通过后，以现有错误路径传播 storage gate 的 Err。压缩是已提交事实，后继准入失败不能改写它。测试利用已有 mock provider 与 TimelinePersistence ACK 边界，禁止 sleep 猜测时序。

## Risks / Trade-offs

共享存储故障由测试显式注入，证明一条真实代码路径，不是原反馈的环境证据。若取消竞态审计发现不同根因，先以失败用例确认，再在同一受影响边界修复；无关架构问题独立登记。
