## Evidence
旧实现两项回归均失败：超过读取上限1字节的完整输出仍返回计划；含NUL条目正文仍返回计划。修复后 `cargo test --locked --offline -p config --lib managed_text --quiet`：31 passed，2.76s。

边界测试实际应用恰好4 MiB的计划，再次规划为NoChange；超过1字节在plan拒绝。已有4 MiB原文追加短条目拒绝且源保持，无事务材料。NUL正文在已有/缺失源均拒绝；注释前缀NUL也被构造器拒绝。所有操作使用隔离临时文件。

全量规范16项严格通过，归档后再次验证。输出预算检查发生渲染之后，不声称限制所有规划阶段内存分配。
