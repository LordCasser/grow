## Context

ChatState 的 ImageProjection 保留原图及可复用描述，主采样按 transport 负能力选择文字；协调问答直接 `project_portable_history`，不会自动选择描述。`recap.rs::handle_side_question` 已正确等待 `image_projection_gate` 并在请求副本选择描述。协调的现有 client 准备与 model 读取也分两次，不能据后一次配置决定图片资格。

## Goals / Non-Goals

在 shell 现有 Sideband 边界修复冻结请求。图片转换、能力记录及 durable ACK 仍由前台与 ChatState 所有。协调回答没有新的工具、前台写入或图片恢复权限。不改变 sampler wire 协议及全局首次乐观发送策略。

## Decisions

将 `/btw` 的冻结与图片选择提取到 `sideband.rs` 的共享方法，返回原 TimelineRangeRef 和 portable items。持有现有图片读锁期间 materialize 和选择已确认描述，再释放；保留完整工具对。已知不支持且仍缺描述时返回原有错误，不删除图片掩盖缺口。

协调完成既有授权后，从单份 prepared config 构建 client、model 与 ModelImageInputKey；在冻结等待外 select inquiry 与 session cancellation。请求继续沿已有 InfoRequest Sideband 审计、用量结算与终态路径执行。

## Risks / Trade-offs

未知路由且图片从未转换时仍可能首次 400，这是现有乐观能力策略，不由只读咨询越权修复。真实 HTTP 回归覆盖三种 backend、用户附件与工具图片；转换 ACK、失败及取消由现有持久化夹具控制，验证主 Surface 不受咨询修改。沿真实入口验证 Tier 2 行为，并对失败和取消路径作对抗审查。
