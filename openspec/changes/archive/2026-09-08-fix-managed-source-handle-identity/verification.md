## Evidence
代码证据：旧 read_source 从路径 metadata 获取 mode/identity，之后独立 File::open 读取内容。现在 read_source_file 从同一 File 获取元数据并执行有限读取，打开对象类型与大小再次检查。

`cargo test --locked --offline -p config --lib managed_text --quiet`：29 passed，2.74s。新回归持有600模式原文件句柄，将路径原子替换为644模式且不同正文的文件，要求旧句柄读取等于完整原状态、新路径状态为完整新状态；另直接传入目录句柄必须拒绝。既有大小、NUL、路径、事务、备份、冲突及验证器测试通过。

没有使用不确定的线程竞速，也没有声称对原地并发写入提供原子快照。Windows未实机验证。全量规范16项严格通过，归档后再次校验。
