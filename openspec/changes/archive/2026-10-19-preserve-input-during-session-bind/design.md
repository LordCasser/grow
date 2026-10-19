## Context

队列出队不等于 ACP 请求已接纳。发送前，Pager 仍持有原 prompt identity，并需将精确输入保存到本地单源草稿。绑定 session 会把 cwd key 迁移到 session key，旧恢复数据不得覆盖这一新责任。

## Decisions

复用现有 DraftSnapshot、worker 顺序和 checkpoint ACK。核对出队输入的捕获及恢复条件；只在没有更新的本地输入责任时应用旧草稿。保留失败时不发送、原身份重试和迟到结果绑定校验。不得新增旁路发送或第二份草稿存储。

## Validation

用真实草稿 worker 复现 cwd 草稿、session 绑定、首输入出队及 checkpoint 的顺序，核对保存的原 identity/正文。再运行既有草稿竞态测试、普通 resume 与 root/嵌套树挂起 PTY。仅编译通过不等于问题关闭。
