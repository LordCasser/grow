## Why

已有图片 fallback 回归缺少确定性的 OCR 整链路、取消边界和新 actor 的实际冷恢复请求证据。用户要求补齐，重点证明已完成描述可复用以及取消后不会继续发起辅助视觉请求。

## What Changes

- 为 OCR 提供仅测试可见、task-local 的本地可执行文件替身，不改生产 tesseract 路径或进程预算。
- 串联 primary 拒绝、auxiliary 失败、OCR 成功/失败、durable projection 和 primary 重试。
- 覆盖取消发生在辅助请求准入前及首组辅助请求运行中；验证后续组和 primary 不再发送。
- 修复全局 revision 相等条件及辅助路由可用性对描述复用的错误阻断，保留来源与新投影校验。
- 用实际 fallback 产生的 Timeline/Sideband JSONL 与全新 actor 验证投影前 crash window 和已投影历史的描述复用；仅修复回归确认的缺陷。

## Impact

Shell 图片恢复、描述存储查询和测试 harness；不替换安装的 Grow，不改 live sessions。并行工作树改动不纳入本 change。
