# 设计

`ScrollbackState::append_entries_from` 已是 cursor reconnect 合并旧 transcript 与 tail 的统一入口。合并前按图片 `operation_id` 建立旧行索引；遇到 tail 中同一 operation 时更新旧行内容和运行索引，并移除 tail 副本。已终态旧行不被迟到的进度重开；进行中的旧行接受 tail 终态。原条目 identity 与原有 native commit 状态保留。

验证通过真实 reload 操作序列检查唯一行、原 EntryId、running 状态、终态内容和终态后的迟到 progress。
