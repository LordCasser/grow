# Design
命令 gate_json_to_decision 返回 Allow 后仅 exit 0 直接返回，否则继续现有 exit-code ladder；exit 2 Deny，其余 Failed。HTTP Allow 分支先检查 status.is_success，非成功返回 Failed(status)，Deny 仍在任意状态下生效。

真实 dispatcher 回归让 command 打印 allow 后 exit 1，并设置 on_failure=block，验证最终拒绝且记录 Failed。解析矩阵覆盖非零/非2xx，显式拒绝和成功允许保持。
