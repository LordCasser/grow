## Tasks

- [x] 核对 adapter/lease 生命周期和 load 内补写顺序；在 release 基线中独立测 prefix 读取/解码/fold 次数与耗时。
- [x] 初次 writer load 同次生成 Timeline 和既有 LedgerPrefix，仅在成功验证后安装到原 adapter；不新增跨 owner 复用接口。
- [x] 保留 append 锁与原失效回退，验证同长度修改、文件替换、truncate、尾片及路线补写；审查读取前后 stamp 对读中修改的拒绝。
- [x] 验证 artifact 失败无状态泄漏、跨 lease/只读隔离、重复 load、lost ACK 与 file/directory sync 失败；同夹具比较前后 release ready/首屏/内存。
- [x] 记录性能与锁开销，执行 OpenSpec strict 校验后再归档；清理本项构建残留。
