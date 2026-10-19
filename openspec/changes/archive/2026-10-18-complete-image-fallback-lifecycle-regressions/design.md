## Approach

复用现有 MockInferenceServer、ImageProjectionPersistence ACK gate、Sideband JSONL 存储与 ChatStateActor::spawn_from_timeline。OCR 的替身通过 cfg(test) task-local 显式传给现有进程运行器，不改全局 PATH。取消使用真实 foreground 任务 abort handle 和 cancel_running_task，服务器以请求 barrier 控制时序。冷恢复使用新 ImageDescribeCache 和从磁盘校验重建的 Timeline，不把旧 cache 注入新 actor。

先运行新增回归。若发现生产缺陷，记录失败证据并更新 delta/design 后修复；其余保持最小测试闭环。每个测试要断言 HTTP 请求数、请求内图片/描述、durable projection、原图证据或取消后的后续请求缺席，而非只断言内部返回值。

## Confirmed cold-recovery defect

新增整链路回归在描述完成、投影未提交的窗口稳定失败：恢复后新输入改变全局 Surface revision，`select_completed_image_description` 以全局 revision 相等判断把仍然有效的源局部中性描述丢弃，导致重复辅助请求；投影存储验证也包含相同的过强相等条件。

保持 source SurfaceId、prompt、completed result、source/evidence range 和原图 fingerprint 的既有校验；只允许同一当前 branch 原始来源的历史 revision 不晚于请求 revision，且请求 revision 不超出已持久化父 Timeline。投影自身仍严格绑定当前 revision。同步更新历史查询和存储 provenance 校验，避免内存投影成功但下次加载失败。更换来源、prompt、未来 revision、损坏/未完成结果继续拒绝。

## Reuse before auxiliary route admission

历史完成描述的查询原本在 auxiliary route `Some` 分支内。冷恢复时关闭辅助配置，即使持久化描述仍有效，也会绕过查询进入 OCR/removal。将源局部 prompt/cache key 准备、批量 durable lookup 和 cached shadow 使用提升到路由可用性之外；只为仍未命中的组解析并调用当前辅助模型。全部命中时不初始化辅助 client 或发送正在调用辅助模型的状态；未命中组继续原有 auxiliary → OCR → removal 顺序。
