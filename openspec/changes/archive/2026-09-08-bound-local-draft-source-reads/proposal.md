# Why
草稿恢复File::open可能等待FIFO；metadata后只take(limit+1)，未拒绝实际超限，增长文件可能以合法JSON通过解析。

# What Changes
Unix非阻塞打开，检查同句柄为普通文件；实际读取超出256KiB时按既有超大草稿策略隔离，不解析。

# Impact
仅local draft读取，保留正常符号链接、缺失及损坏记录行为。不改变恢复/发送所有权、写入与隔离目录保留策略。
