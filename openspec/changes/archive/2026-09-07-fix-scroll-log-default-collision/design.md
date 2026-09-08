## Design
default_log_path 继续选择 grow_home/logs，调用接收目录与时间的私有路径生成函数，后缀增加已有 uuid::Uuid::new_v4。路径生成不创建目录或文件，保持 lazy-open。测试用私有目录和固定时间生成两条路径，再通过真实 Recorder 写入，验证互不覆盖。

## Boundaries
UUID 提供实践上的唯一性，不宣称对抗恶意路径抢占。显式路径仍 File::create；同步写入、无限记录和错误后状态可见性分别审计，不混入此修复。
