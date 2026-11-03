删除资格的所有者仍是 JsonlStorageAdapter。目标 canonical Spawn/Seed 指向其他 session 时拒绝；其他 session 的 Spawn/Seed 指向目标时也拒绝。summary.parent_session_id 不参与该判定，保留供 fork 展示。拒绝信息继续给出关联 identity，不扩成递归清理。

Tier 3：真实 copy_session_data 创建同 cwd/cross-cwd fork，分别删除 fork 或 source 后验证另一份 Surface/summary 可读取；canonical Seed 双向拒绝沿用既有链 fixture，再新增 incoming Spawn 无 Seed/无 summary parent 的窗口测试。所有操作使用临时根，原用户数据不参与。完成后再处理长历史预算与删除失败页面收敛。
