## Decision

复用 `last_idle_flush_conversation_len` 作为唯一成功 checkpoint，不增加独立活动时间或重试状态。run loop 的 idle arbiter 与 SessionActivityTracker 变化会重新开始 timer；到期时同时检查 `ForegroundState::Idle` 和 session activity idle。失败不写 watermark，后续完整 idle interval 可重试同一历史。compaction 现有 ConversationReset 仍重置该长度基线。

启动时将零秒 timeout 过滤为 disabled，避免零间隔 Sleep。timer 只负责到期通知；MemoryFlush 的 bool 语义为成功 checkpoint（包括写入和确认为无内容）才可推进水位。

## Boundaries

本 change 不调整 flush 内容选择、存储协议、手动 flush、pre-compaction admission 或 memory toggle。idle activity 复用既有 run loop 通知和 session activity ownership。
