# Capture and recovery ownership

## 已核事实与决定

父核 `handler.rs` Unix O_TRUNC 与 Windows CREATE_ALWAYS、`lib.rs::check_previous_crash` read→两份 report→unlink、CLI 单次恢复及 PB-07 已归档持久化保证。signal handler 只用预分配内存与预打开 raw handle，不能在 signal 中分配随机名、解析目录或获取新锁。

安装时用现有 tempfile/标准库在 crash_dir 创建唯一私有 raw，命名须可精确识别本 capture（如 capture-<pid>-<random>.bin），在普通启动上下文取得原 File 的 `try_lock` 后把同一句柄交给 handler；不 reopen、不 truncate 他人文件、不用 PID 充当唯一身份。锁在 handler 最后写入结束并关闭句柄，或进程终止时释放。Unix需保 close-on-exec，不让后续子进程延长锁寿命。保原 platform handler/terminal 注册，不重做 signal 处理框架。标准库 File 锁已有 diagnostics 模块用法，无需新增依赖。

恢复先在专用恢复锁文件取得非阻塞排他锁，再枚举识别出的 raw（含既有 last-crash.bin），逐个打开、try_lock 原句柄并读取；WouldBlock 表示 live/其他 owner，跳过且不读不删。其它 I/O 错误明确传播。候选顺序确定，单次 API 继续返回最多一个 CrashReport，CLI 循环调用直到 None，避免漏掉多个已结束进程。恢复锁被占时返回本次无可取得候选，不声称其它进程已恢复。`check_previous_crash` 不根据 PID/时间猜 stale，也不把目录中任意 .bin 当本功能文件。

同一 raw claim 从读取保持到原 primary+history 发布和 raw 消费完成。非空 malformed、read/parse/publication/barrier 失败保原 bytes；空文件只能在取得 claim 后清理。历史名称含原 timestamp 与 capture 身份，不让同秒两份覆盖，MAX_HISTORY 仍适用；恢复全事务串行，防止另一恢复 prune 正在发布的档案。primary 仍为最近一次成功恢复的展示入口。无需新数据库、claim 状态表、rename protocol或跨平台 PID 探测。

## Coder Task Spec

唯一结果：多进程开启 capture 时，每个 live/已 crash 的 raw 独立保全，恢复只能消费已取得 claim 的原文件。write scope 为 crash-handler 这一生命周期实现/测试及 CLI 原恢复入口必要循环和文案；不改 blob schema/symbolicate（PB-06）、权限设置、默认开关或其它 dirty。保留用户既有 PB-07 实现。

Tier3：双/三进程隔离序列，A 安装未 crash，B 安装/退出不能截断 A，A crash 后 C 恢复只消费 A；恢复者并发不能重复 claim 或覆盖同秒历史；live raw 与空文件不被其它启动删除；旧 last-crash.bin 成功和失败恢复；已有 read/parse/primary/history/barrier 失败保真。使用 fixture 自有进程/临时目录，信号和清理仅针对实际 spawn owner。必须有有效旧红及新绿，记录平台范围。必要公开 API 签名保持现 `std::io::Result<Option<CrashReport>>`（单次至多一份，不移除PB-07错误传播），通过 CLI 循环完成多份处理；不增公共 registry。

## Review Plan

父核 install 同句柄锁的寿命、跨平台 raw 移交、恢复 claim 与 report/history 消费边界、同秒身份、CLI 循环终止。独立 reviewer 在本数据完整性改动实现后核决定性 diff 与并发反例；最终与 PB-06 顺序集成再跑 crash/CLI 相邻门禁。cargo 窗口串行，后者必须等本任务释放 crash 源码写入。未跑 Windows 不声称实测。父统一主 spec/docs/backlog/archive。
