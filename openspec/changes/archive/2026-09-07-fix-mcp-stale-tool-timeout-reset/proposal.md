# Why
工具超时分支仍无条件 reset_transport。旧请求等待期间如果其他恢复已经完成，迟到的超时会把新 Ready 连接改回 Pending，导致额外握手并干扰后续调用。

# What Changes
超时 reset 在状态锁内比对失败服务身份，仅当前 Ready 仍匹配才重置；超时仍返回错误且不重试工具。

# Impact
仅超时重置接纳和相关回归，保持主动恢复与错误单次重试行为。
