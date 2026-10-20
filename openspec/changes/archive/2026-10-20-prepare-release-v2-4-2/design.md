# Design

使用下一个 patch 版本 v2.4.2。先提交本会话源码和审计归档，再提交版本/发行说明准备。静态 preflight 需清洁提交和指向 HEAD 的 annotated tag；本地通过后原子推送 main 与新 tag，从 main dispatch 官方 release.yml（全部平台、publish=true）。跟踪到工作流完成，核对公开 release、十个 archive 和 SHA256SUMS、远端 tag 身份，再追加发布验证记录。必要的真实门禁修复独立立项，不移动已经发布的 tag。
