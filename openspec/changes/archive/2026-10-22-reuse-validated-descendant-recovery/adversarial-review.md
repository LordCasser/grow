# 对抗性审查

本项审查以 `production-diff.patch`（相对本任务生产修改前的副本，含 colocated tests）、实际调用方和测试为依据；共享 checkout 中既有改动不作本项新增。没有修改磁盘格式、writer lease、prefix hash、durable ACK、同步级别、就绪屏障或分页。

- **不是把缺失当空树。** 只有匹配 canonical response session、wire shape 与当前已接受 task result 的 payload 才可复用；普通 load 的既有 agent/result gate、resync/reconnect generation 先验收结果。无/坏/外来数据独立完整读取；超预算省略整份，不截断。五个新 UI 用例包含根文件在捕获后消失仍正确补全、foreign fallback、空截点不吸收后来追加、UI 行不能授予遍历及 stale generation 拒绝。
- **cut/hash 不是魔法证明。** 服务端使用实际 replay 的已完整验证 Timeline/history 创建数据；客户端信任 ACP server，如同其他 load metadata。它不重新证明已捕获后磁盘未变，也不授予 writer/执行资格。下一独立恢复仍完整校验；不保留跨 load cache。它不是一个全树原子快照：各后代有自己的单次读取截点。
- **读一次也必须完整验证。** 新 helper 复用原 shared-read pinned entity 与 `validated_timeline`，完整检查记录、fold、receipt owner、blob/Sideband 证据，再调用同一 `into_prepare(None)`。只对最终通知做 lifecycle/control 选择，不选择性跳过正文校验。七个新存储测试覆盖空/缺失、canonical/UI-only 权威、外来 receipt、缺失 mandatory ledger、checksum、顺序及捕获后追加；完整既有 storage/history/persistence/child execution regressions另行留证。
- **展示与执行分工。** canonical parent facts/UI identity intersection、BFS/visited、source-local eventId/cursor/highwater、CWD/worktree/control及原 flat task display 保留。best-effort display 不替代严格 Seed/Result execution/export reader，缺失子节点仍沿原 display 降级，不造成功结果。真正的 nested cold resume/partial release/concurrent claims 作为独立正确性门槛。
- **响应大小有实际限制。** ACP 单行64MiB；可选 metadata 用 JSON escaping 上界限定新增内容约1MiB，超限 fallback。测试用真实控制字符序列验证 escaping 后实际大小及整份省略。此限制只约束新增 wire payload，不声称限制已持有 Timeline/临时 spawn 字符串的全部内存，亦不改变其他 response 字段的预算。
- **不把局部成绩当整体成绩。** root load RPC 原有完整读取/校验/历史交付仍在；首条历史预期基本不变。无 child 时消除的是 UI 收尾重读；每 child 只消除第二次读取/校验，完整 projector 的正文展开仍在。子树基准仅计 Pager display restoration，不计执行恢复，也走独立 root fallback，不能将其比例外推为全流程倍数。真实 PTY 分开 last-history / ready-visible / full-finalize；default-off 与详细 log 分开，阶段不相加。

## 基线修正及未混入的债务

保留三份失败夹具记录：初始 chain 缺 presentation Spawned，第二次误把安全父链当嵌套显示布局，第三次 modern Spawned 未写 producer 稳定 eventId。最后按实际 producer 同时写 canonical Spawn、匹配 Seed、typed presentation 及 source-local eventId，保留全部身份/顺序/重入断言后基线通过。保留完整身份/重入等失败场景，布局断言按实际既有行为修正；失败样本不计性能数据。

无 eventId 的合法 lifecycle presentation 重复恢复可重复添加任务行，是原 forced display replay 的既有问题；与展示路径尚未采用执行级跨实体校验一并登记 backlog。它们未混入本项。既有 CompletedOutputUnavailable 和 release-only debug_assert should_panic 债务不改；后者沿 P0/P1 明确排除同一测试，不计通过。

工具维护：并行/排队构建脚本曾以启动时 identity JSON 覆盖其他组的条目，导致两次检查启动 KeyError（测试未运行）；已改为每组成功后读取最新索引，并从保留 executable 重建完整 SHA 清单。不是运行期故障。

归档收尾：proposal标题缺Why/What Changes的非阻塞提示已按原内容补齐；archive时5/7只剩归档/清理行政任务，完成后全部勾选。前后strict和最终archive验证均通过，原始archive提示未删除。
