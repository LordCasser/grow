## 所有权与边界
get_file_hunk_data 持有不可变 file state；as_str 只对 Full 返回文本。一次调用中的所有 hunk 使用同一对文本。把行表放在现有 diff 模块，生命周期只覆盖本次查询；使用标准 Vec<&str>，不增加 Arc schema 或持久字段。

保持原 generate_hunk_patch 的 public string API，其包装行表后调用同一 formatter；内部批量路径在 map 之前创建同一对表，有任一 Full 时缺失侧仍按空文本。两侧都不是 Full 时不创建 patch；零 hunk 时不扫描正文。

## 验证
Tier1：独立旧函数逐字节 oracle，覆盖替换、插入、删除、多个间隔 hunk、CRLF、Unicode、无末尾换行、空文件，以及实际 actor Full/Binary/TooLarge/Missing 返回。临时 probe 在相同文件与 hunk 上比较重复单入口和一次行表复用；setup 在计时外，输出 drop 一致，预热与原始样本保留。测 128/512/2048 行及接近 1 MiB，多 hunk 和单 hunk，不能将 kernel 收益说成 UI 绘制加速。
