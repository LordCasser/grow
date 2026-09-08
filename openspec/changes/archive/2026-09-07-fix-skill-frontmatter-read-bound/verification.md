## 旧行为复现
frontmatter_reader_bounds_long_lines 在旧实现失败：4096 字节名义上限却读入 2097152 字节单行。

## 修复验证
完整 Cargo tools lib implementations::skills：95 项全部通过。新测试覆盖 2 MiB 单行、UTF-8 探测边界截断、恰好 4096 字节且 EOF 关闭、关闭行换行导致超限不采纳、限额内真实非法 UTF-8 仍返回 InvalidData。

读取组合为 BufReader<Take<File>>，限制在缓冲预取之前。总源读取至多 4097 字节；Vec 容量可能按分配器增长，但不会随文件单行长度无界增长。返回 total_bytes 反映实际有界消费量，不是整文件长度。

构建环境关闭 incremental/dev-debug/test-debug，jobs=2；cargo test --locked --offline -p tools --lib implementations::skills --quiet。未声称 body 加载、描述回退或整个技能链路均有此上限。
