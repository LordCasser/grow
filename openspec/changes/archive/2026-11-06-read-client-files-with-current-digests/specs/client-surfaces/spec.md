## ADDED Requirements

### Requirement: ClientFS 文件操作的摘要与 size 来自同一次当前读取

`workspace.client_fs_stat` 与 `workspace.client_fs_read_file` 对文件的响应 SHALL 在对当前打开文件的同一次流式读取中产生全内容 SHA-256、请求 range 字节与上报 size；SHALL NOT 以 `(size, mtime_ms)` 等元数据元组缓存或复用摘要，SHALL NOT 将预读取的 metadata `len()` 作为文件响应的 size。元数据 SHALL 仅决定节点类型与 mtime 显示。普通并发原地 writer 下本操作不提供文件系统原子快照，该一致性 SHALL NOT 被宣称为瞬时全文件快照。既有 root-confinement、symlink 逃逸排除、read cap 与 binary-safe 编码行为保持。

#### Scenario: 同 size 且保持 mtime 的原地改写

- **WHEN** 文件已被 stat/read 后以相同长度的新内容原地改写，且 mtime 恢复为原值（同毫秒）
- **THEN** stat 与 read_file 返回的 hash 等于新内容全量 SHA-256，read_file 的 range 内容为新字节，size 等于新内容长度，mtime_ms 显示不变。

#### Scenario: 同 size、同毫秒不同纳秒的改写

- **WHEN** 文件以相同长度新内容改写，且 mtime 恢复到同一毫秒内的不同纳秒值
- **THEN** stat 与 read_file 返回的 hash、range 字节与 size 反映新内容，mtime_ms 显示不变。

#### Scenario: atomic replacement 保持元数据

- **WHEN** 以同长度临时文件 rename 原子替换目标文件，并将 mtime 恢复为原值
- **THEN** stat 与 read_file 返回的 hash 等于替换后内容的全量 SHA-256，size 等于替换后长度。

#### Scenario: 既有边界行为保持

- **WHEN** 越权路径、symlink 逃逸、超出 read 预算的请求或非 UTF-8 内容到达 client_fs 操作
- **THEN** root-confinement 拒绝、symlink 逃逸拒绝、read cap 截断与 binary-safe 编码降级行为与既有契约一致。
