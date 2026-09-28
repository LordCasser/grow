## Context

当前调用顺序是 Surface clone → goal/description 投影 → 图片预算 → `ContinuationLane::reconcile_projection` → 请求编码。`observed_projection` 比较旧 item hash，变化会撤销 native spans 并更新 epoch；key 又包含该 epoch。预算算法没有跨请求状态，因此其低水位只对一次副本有效。

约 15 MiB/张的反例：第 4 张时约 60 MiB，淘汰 1–3；第 5 张时又从约 75 MiB 开始，淘汰 1–4。已有 `eviction_keeps_newest_and_is_idempotent` 只重用被修改的 slice；actor 图片测试尚未覆盖首次回收后的持续追加。该差异属于注释/测试保证范围与实现的偏差，现行主规范未承诺低水位的跨轮稳定性。

## Goals / Non-Goals

目标是让相同投影域中的批量回收真正保留跨轮余量，并继续在最终 wire 层拒绝超大请求。这里只保证本地请求选择稳定；远端缓存命中、TTL、tokenization 和节点路由交给 provider 验证 change。

原始 Timeline、当前图片描述的 durable 语义和 native 安全边界继续由各自所有者负责。预算选择不承载 KV、opaque reasoning 或新的图片正文副本。

## Decisions

### 1. 选择归属 ChatStateActor，独立于 native epoch

在 actor 的既有请求投影状态中保留已淘汰图片的身份。键使用 `SurfaceId + part 位置`，并校验对应源 item/图片仍匹配，禁止只保存易漂移的数组下标。只保存身份与小型校验信息，不复制 base64；源身份改变就重新建立选择。

不把选择放进 `ContinuationLane::reset` 管理的 native spans。一次真实预算推进会合法触发 native reset，但该 reset 不能同时删掉刚建立的预算选择，造成下一轮恢复原图的循环。单纯 native 协议恢复也保留有效预算选择。

### 2. 先重放选择，再测量有效请求

1. 完成 Surface、goal 和图片描述选择，校验当前投影域。
2. 将仍有效的既有图片选择替换为相同 placeholder。
3. 对该有效副本执行既有 `conversation_body_bytes`；低于 47 MiB 就保留选择。
4. 达到 47 MiB 才按 oldest-first 继续回收至 25 MiB，并扩展选择集合。
5. 用最终 items 执行 continuation reconcile，再构造请求；成功构造后保留本轮选择。失败的局部构造不发布半份选择。

三协议共享这段 provider-neutral 选择。`source_projection.image_budget` 记录完整生效选择以及本轮是否推进，按最终 Surface 顺序派生索引；不要把“本轮新增淘汰为零”记录成“没有既有淘汰”。实现时同步明确 before/after 字段的测量阶段。

### 3. 生命周期由投影域决定

| 变化 | 预算选择 |
| --- | --- |
| 同源 Surface 纯追加、重复 build、同路由原生协议 reset | 保留 |
| 同路由 output limit、temperature、effort 等不改变图片呈现的更新 | 保留 |
| rewind、compaction 或其他旧 Surface 身份/内容替换 | 清除，按新投影重建 |
| route 替换、goal 过滤域或 image/description 呈现域切换 | 清除，按新投影重建 |
| fork、新 actor 冷恢复 | 从各自当前 Surface 建立新选择 |
| resident reconnect | 沿用存活 actor 选择 |

新域允许从当前 Surface 重新选择图片，但不能从 immutable 历史中复活已被 durable `ImageProjection` 移除的图片。冷恢复不恢复 native spans，也不保证延续旧进程的缓存热度。

### 4. 预算失败沿既有路径处理

图片回收不能解决超大纯文本或工具定义。最终 wire body 检查仍由 sampler 执行；剩余内容仍超限时返回原有错误，不能绕过上限、无限重试或隐式扩大摘要范围。该选择不写新 Timeline 事件，不增加持久化事务。

## Risks / Trade-offs

- 将旧 Surface 下标当身份会替换错误附件：以因果身份校验，覆盖工具结果多 part、rewind 和压缩后的重新定位。
- native reset 误清预算选择会重现问题：把“预算选择仍有效、native 已失效”作为独立回归。
- 临时状态使冷恢复后的图片选择可能变化：这是明确的边界，证据只解释实际构造过的请求，不承诺跨进程热缓存。
- 新域切换与请求构造交错：所有状态在现有 actor mailbox 内更新，测试使用真实 build/control 入口。

## Validation

先添加首次回收后至少两次追加的失败回归，再实现。分别使用少量大图片与多张小图片，保证存在至少一轮“应用旧选择后未达高水位”。核对旧 part、epoch、key 和原始 Timeline，而不只检查最终 body 小于上限。三协议验证附件顺序与文本替代，重跑相关 portable/native 场景。

验证命令按受影响范围选用 `cargo test --locked -p chat-state --lib image`、`cargo test --locked -p sampling-types --lib image`，新增 lifecycle 测试按实际名称单独运行。实现完成后把精确命令、结果和未执行范围写入 `verification.md`；按项目约束控制和清理本次产生的 Cargo 残留。
