# F3 review

父核对整合后的六个Shell文件、ChatState提供方和独立审查报告 `01a1af56-cba6-7d72-8daf-22829fe2e744`。此前review任务 `01a1af50-f7ce-7db2-902a-65b11cff176e` 在零工具调用后失败，没有可采纳结论。

同请求重试先查canonical lifecycle、只读命中payload、排除服务端冻结的skill引用后核对不可变内容，符合状态归属。queue先发送AdmissionReplay再发送persist_ack，ACP biased选择应答优先；重试在normal completion、usage和roster路径前返回。已知穷尽匹配缺臂、返回值类型及测试LocalSet前提已由定向编译暴露，按相邻调用方修正，不能把静态阅读当编译证据。

独立审查的两个发现分别核对如下：

- `submit_input_durably` 返回 `created=false` 后未再次核payload：该假设被提供方现契约排除。`chat-state/src/actor/mod.rs` 的SubmitInputDurably只在既有input_id、intent、payload_ref全部精确相等时返回false，不同内容直接Invalid；每Session另有writer lease，Shell同actor有admission gate。不能用一个违反提供方契约的fake handle证明可达缺陷，不增加重复防护。
- queue编辑的replay分支只返回内存行version：已接纳但尚未应用replacement时可能假成功。当前修正边界仅为确认成功条件：行input_ids已经包含admitted.input_id才返回version，否则返回既有queue_changed_after_admission错误；不在本项重建队列、不补做F2的排序与冷恢复。

F3验收所需actor相关套件、上述queue编辑反例及ACP真实入口均已通过，详见verification.md。ACP在首次prompt仍运行时以同promptId重试同payload，返回原inputId/canonical状态；改正文对照为冲突，未重复usage/roster或canonical执行事实。该验收仅关闭F3。
