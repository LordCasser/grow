初始 fork placeholder 本来就有 pending_fork_banner（parent SID）；forked_from 只用于本地返回点，不能承担请求分类。worktree/plain startup failure 若带 pending fork banner 复用既有 fork failure handler；其 bound-session guard 仍拒绝迟到初始失败。用实际 fork dispatch、source close ACK、WorktreeSessionFailed 顺序验证 directive 留在可关闭草稿。

SessionLoadFailed 对 pending fork 在终止 replay batch 后释放失败 binding，复用 fork failure handler，不先清除 pending directive/banner。源页关闭测试需独立 draft store，避免相同假 session ID/cwd 从其他测试恢复既有草稿；只增加 test-only store 注入。
