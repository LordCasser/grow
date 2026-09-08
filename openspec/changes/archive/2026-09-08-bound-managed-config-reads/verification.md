## Evidence
计数读取器以原先无界 read_to_end 方式读取128字节、预算16时，回归失败 read beyond the probe budget；改为 take(limit+1) 后实际最多17字节并返回超限错误。该回归测试读取算法，不依赖不确定的真实文件增长时序。

最终 `cargo test --locked --offline -p config --lib managed_text --quiet`：28 passed，2.72s。包含空/恰好预算/超限消费计数，发布后外部增长的原有/新建文件 ×3时点，回滚后再增长返回恢复错误并保留原始备份。源 metadata 的4 MiB预检仍在，三个生产读取位置均调用有限读取函数，source/transaction 不再使用 fs::read。

全量规范16项严格通过；归档后再次校验。未实测慢网络文件系统总期限，不声称时间上限；文件身份/open 竞态未混入此变更。
