## 不变量与实现边界
1. 普通取消向现有 BackgroundCompaction.cancel 发信号，保留 worker slot，不在 root inbox 中等待未完成 worker；发布边界检查同一 token。
2. 手动压缩排队时，普通 turn 必须在安全边界让出给 idle arbitration，不能把 skipped 自动压缩当作取得进展反复开 Step；保留手动请求及用户输入事实。
3. 摘要范围仍由原 Surface identity/完整历史边界定义，预算按实际摘要投影计数。排除不会发送的 reasoning/派生回忆，但不得因此扩大 replacement 到未覆盖证据或拆散工具组。预算缩减重试使用同一计数口径。
4. Shell 摘要 adapter 在共享摘要生成器接纳正文之前，检查 provider 终态；截断、超窗、refusal、pause 不成为可替换历史的成功摘要。沿现有 Sideband 结算与错误分类结束/有界重试。
5. formatter 仅移除可识别的前导 scratchpad，不能因为正文不是数字开头便删掉任意 analysis closer 之前的内容。
6. 只有显式的成功终态（`stop`、`end_turn`、`stop_sequence`）可通过 Shell 摘要 adapter；缺失或未知终态按有界 transient 失败处理。Responses EOF 不得伪造 `stop`，只有 `response.completed` 记录成功终态。

## 验证
保留审计中六个失败探针的源码/结果，转为正式回归并加正常路径反例。检查恢复、取消、route 变化、压缩失败终态和 source identity；运行受影响库测试及 OpenSpec 校验。修复前日志只是本地模拟证据，不声称重放用户真实 session。
