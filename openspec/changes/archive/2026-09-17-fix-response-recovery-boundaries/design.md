## Context

主规范仍是已归档契约；尚未归档的 `reconcile-response-replay-projection` 描述现有实现意图。本 change 为审计确认的四个边界增加明确约束，不据未完成的旧 tasks 声称全部原设计已经验收。

## Decisions

1. Rewind 必须保留每个所选条目的原始响应来源，而非只复制 admission event 集合后把所有来源重置成 rewind event。复用 branch provenance；覆盖重复 rewind、compaction 前缀以及 cold fold，不放行已切走 projection。
2. Fork 是新 lineage。先按父 Timeline 核对和展开 response projection，再把其 text/thought 作为普通继承历史 ACP 行复制，移除父 response/candidate 身份元数据。不伪造子 request/admission；普通工具更新顺序保持，quarantine 不复活，截断和 fork_filter 保持既有含义。
3. Resident gateway mute 不停止 actor。初次物理 snapshot 遇到 event seq 晚于已验证 Timeline snapshot 的 projection 时，在该行之前截止；返回同一 byte offset 供 delta 读取，而不是过滤掉该行后越过它。完整记录和后继更新留在 delta，已在初次回放合成的已知 projection 继续去重。Cold load 仍对未知/失效 identity 使用既有规则。测试用确定性调度窗口覆盖初次 snapshot 和后续 delta。
4. Exact payload 的可读性只能证明去重身份，不能证明 durability。对已有 exact 行及写入错误后的 exact reconcile，成功前重新同步同一 contained updates 文件和目录；同步失败返回错误并保留原记录供 exact retry。已完成 durable append 后的 summary bookkeeping 失败仍可按 exact 记录确认。测试注入真实 append 路径的 file/directory sync seam，覆盖 persistent failure、恢复后 retry、不重复 append 和 ACP 独立行。

## Scope and Verification

只修改上述边界及必要回归。先运行新回归展示旧实现失败，再运行修复后的 ChatState、Shell storage/persistence/replay 和相关 actor 定向测试。changed-file rustfmt 避免递归改动兄弟模块。运行严格 OpenSpec 与 diff 检查，完成后归档本 change，不关闭未完成的全仓 review 或旧 projection change。
