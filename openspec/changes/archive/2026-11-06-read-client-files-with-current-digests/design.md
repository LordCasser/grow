# Design: read-client-files-with-current-digests

## 决策

1. 直接删除 memo，而非加强键（mtime 纳秒、inode、generation）。任何元数据元组都证明不了"内容未变"：同长度原地改写后恢复 mtime、同毫秒不同纳秒改写、atomic replacement（换 inode 保 mtime）都能保持 `(path, size, mtime_ms)` 不变。没有可信内容代际来源时，唯一正确做法是不用元数据复用摘要。不预建 watcher generation 协议或缓存实体。
2. stat 与 read_file 都走 `handle::stream_hash_and_range`（单次 `File::open`、逐块同时喂 SHA-256 并截取 range，返回实际 streamed 字节数）：
   - stat 以 `(offset=0, length=0)` 全量流读，`size` 取返回的 streamed 值，hash 即全内容摘要；
   - read_file 以请求 offset/clamp 后 length 流读，hash、range 字节、`size` 三者同源；
   - metadata 仅决定 stat 的 `node_type`/`mtime_ms` 显示与 read_file 的目录错误分支，不参与摘要或 size。
3. read_file 旧实现的第二处缺陷随 memo 一并消失：memo 命中时另开一次只读 range 的读取会返回新字节配旧 hash/旧 size；现在 range 永远与 hash 同遍。
4. 代价声明：重复 stat 会重复全文件 hash I/O（原 memo 避免的部分）。这是正确性必要成本，量化优化不在本项范围。

## 保持不变

`list` 全部行为（排序、分页、`MAX_LIST_COLLECT`、symlink 逃逸排除）、root-confinement 与 `resolve_service_path` 判定、`clamp_read_length` read cap、`encode_chunk` binary-safe 编码、stat 的 ENOTDIR/缺失语义、read_file 的目录错误语义、`ext_fs`/`get_files`/`session::file_system` 算法。

## 不做 / 延期

并发原地 writer 下的文件原子快照：普通读取无法提供，本项不宣称；watcher generation、LRU 缓存、mtime/inode guard 均无依据，不加。`get_files`（shell 侧）仍报 metadata size，属另一 owner，不动。
